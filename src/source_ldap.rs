//! LDAP -> Famedly Zitadel sync tool.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use ldap_poller::{ldap::EntryStatus, ldap3::SearchEntry, Cache, Ldap, SearchEntryExt};
use tokio::sync::mpsc::Receiver;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

use crate::{
	config::{AttributeMapping, FeatureFlags, SourceLdapConfig},
	user::{StringOrBytes, User},
	Source,
};

/// LDAP sync source
pub struct SourceLdap {
	/// LDAP configuration
	ldap_config: SourceLdapConfig,
	/// Optional set of features
	feature_flags: FeatureFlags,
	/// Where to cache the last known LDAP state
	cache_path: PathBuf,
}

impl Source for SourceLdap {
	fn new(config: &crate::Config) -> Result<Self> {
		Ok(Self {
			ldap_config: match config.source_ldap.clone() {
				Some(ldap) => ldap,
				None => bail!("LDAP configuration is missing"),
			},
			feature_flags: config.feature_flags.clone(),
			cache_path: config.cache_path.clone(),
		})
	}

	async fn get_all_changes(&self) -> Result<(Vec<User>, Vec<(User, User)>, Vec<String>)> {
		let cache = read_cache(&self.cache_path).await?;
		let (mut ldap_client, ldap_receiver) = Ldap::new(self.ldap_config.clone().into(), cache);

		let is_dry_run = self.feature_flags.dry_run();
		let cache_path = self.cache_path.clone();

		let sync_handle: tokio::task::JoinHandle<Result<_>> = tokio::spawn(async move {
			ldap_client.sync_once(None).await.context("failed to sync/fetch data from LDAP")?;

			if is_dry_run {
				tracing::warn!("Not writing ldap cache during a dry run");
			} else {
				let cache = ldap_client.persist_cache().await;
				tokio::fs::write(
					&cache_path,
					bincode::serialize(&cache).context("failed to serialize cache")?,
				)
				.await
				.context("failed to write cache")?;
			}

			tracing::info!("Finished syncing LDAP data");

			Ok(())
		});

		let (added, changed, removed) = self.get_user_changes(ldap_receiver).await?;

		sync_handle.await??;

		Ok((added, changed, removed))
	}

	async fn get_removed_user_emails(&self) -> Result<Vec<String>> {
		anyhow::bail!("Not implemented!");
	}
}

impl SourceLdap {
	/// Get user changes from an ldap receiver
	pub async fn get_user_changes(
		&self,
		ldap_receiver: Receiver<EntryStatus>,
	) -> Result<(Vec<User>, Vec<(User, User)>, Vec<String>)> {
		ReceiverStream::new(ldap_receiver)
			.fold(Ok((vec![], vec![], vec![])), |acc, entry_status| {
				let (mut added, mut changed, mut removed) = acc?;
				match entry_status {
					EntryStatus::New(entry) => {
						tracing::debug!("New entry: {:?}", entry);
						added.push(self.parse_user(entry, None)?);
					}
					EntryStatus::Changed { old, new } => {
						tracing::debug!("Changes found for {:?} -> {:?}", old, new);
						changed.push((self.parse_user(old, None)?, self.parse_user(new, None)?));
					}
					EntryStatus::Removed(entry) => {
						tracing::debug!("Deleted user {}", String::from_utf8_lossy(&entry));
						removed.push(String::from_utf8(entry.clone())?);
					}
				};
				Ok((added, changed, removed))
			})
			.await
	}

	/// Construct a user from an LDAP SearchEntry
	pub(crate) fn parse_user(&self, entry: SearchEntry, idp_id: Option<&str>) -> Result<User> {
		let status_as_int = match read_search_entry(&entry, &self.ldap_config.attributes.status)? {
			StringOrBytes::String(status) => status.parse::<i32>()?,
			StringOrBytes::Bytes(status) => i32::from_be_bytes(
				status.try_into().map_err(|_| anyhow!("failed to convert to i32 flag"))?,
			),
		};
		let enabled = !self
			.ldap_config
			.attributes
			.disable_bitmasks
			.iter()
			.any(|flag| status_as_int & flag != 0);

		let first_name = read_search_entry(&entry, &self.ldap_config.attributes.first_name)?;
		let last_name = read_search_entry(&entry, &self.ldap_config.attributes.last_name)?;
		let preferred_username =
			read_search_entry(&entry, &self.ldap_config.attributes.preferred_username)?;
		let email = read_search_entry(&entry, &self.ldap_config.attributes.email)?;
		let user_id = read_search_entry(&entry, &self.ldap_config.attributes.user_id)?;
		let phone = read_search_entry(&entry, &self.ldap_config.attributes.phone).ok();

		Ok(User {
			first_name,
			last_name,
			preferred_username,
			email,
			ldap_id: user_id,
			phone,
			enabled,
			needs_email_verification: self.feature_flags.require_email_verification(),
			needs_phone_verification: self.feature_flags.require_phone_verification(),
			idp_id: idp_id.map(ToOwned::to_owned),
		})
	}
}

/// Read an attribute from the entry
fn read_search_entry(entry: &SearchEntry, attribute: &AttributeMapping) -> Result<StringOrBytes> {
	match attribute {
		AttributeMapping::OptionalBinary { name, is_binary: false }
		| AttributeMapping::NoBinaryOption(name) => {
			if let Some(attr) = entry.attr_first(name) {
				return Ok(StringOrBytes::String(attr.to_owned()));
			};
		}
		AttributeMapping::OptionalBinary { name, is_binary: true } => {
			if let Some(binary_attr) = entry.bin_attr_first(name) {
				return Ok(StringOrBytes::Bytes(binary_attr.to_vec()));
			};

			// If attributes encode as valid UTF-8, they will
			// not be in the bin_attr list
			if let Some(attr) = entry.attr_first(name) {
				return Ok(StringOrBytes::Bytes(attr.as_bytes().to_vec()));
			};
		}
	}

	bail!("missing `{}` values for `{}`", attribute, entry.dn)
}

/// Read the ldap sync cache
pub async fn read_cache(path: &Path) -> Result<Option<Cache>> {
	Ok(match tokio::fs::read(path).await {
		Ok(data) => Some(bincode::deserialize(&data).context("cache deserialization failed")?),
		Err(err) => {
			if err.kind() == std::io::ErrorKind::NotFound {
				tracing::info!("LDAP sync cache missing");
				None
			} else {
				bail!(err)
			}
		}
	})
}
