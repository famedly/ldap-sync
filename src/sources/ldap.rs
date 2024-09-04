//! LDAP -> Famedly Zitadel sync tool.

use std::{
	fmt::Display,
	path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Result};
use ldap_poller::{
	config::TLSConfig, ldap::EntryStatus, ldap3::SearchEntry, AttributeConfig, Cache, CacheMethod,
	ConnectionConfig, Ldap, SearchEntryExt, Searches,
};
use serde::Deserialize;
use tokio::sync::mpsc::Receiver;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use url::Url;

use crate::{
	config::FeatureFlags,
	user::{StringOrBytes, User},
	FeatureFlag,
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

impl SourceLdap {
	/// Create a new LDAP sync source
	pub fn new(config: &crate::Config) -> Result<Self> {
		Ok(Self {
			ldap_config: match config.source_ldap.clone() {
				Some(ldap) => ldap,
				None => bail!("LDAP configuration is missing"),
			},
			feature_flags: config.feature_flags.clone(),
			cache_path: config.cache_path.clone(),
		})
	}

	/// Get all changes from the LDAP server
	pub async fn get_all_changes(&self) -> Result<LdapChanges> {
		let cache = read_cache(&self.cache_path).await?;
		let (mut ldap_client, ldap_receiver) = Ldap::new(self.ldap_config.clone().into(), cache);

		let is_dry_run = self.feature_flags.is_enabled(FeatureFlag::DryRun);
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

		Ok(LdapChanges {
			new_users: added,
			changed_users: changed.into_iter().map(|(old, new)| ChangedUser { old, new }).collect(),
			deleted_user_ids: removed,
		})
	}

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
			needs_email_verification: self.feature_flags.is_enabled(FeatureFlag::VerifyEmail),
			needs_phone_verification: self.feature_flags.is_enabled(FeatureFlag::VerifyPhone),
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

/// Return format from LDAP poller
pub struct LdapChanges {
	/// New users
	pub new_users: Vec<User>,
	/// Changed users
	pub changed_users: Vec<ChangedUser>,
	/// Deleted user IDs
	pub deleted_user_ids: Vec<String>,
}

/// A user that has changed returned from the LDAP poller
pub struct ChangedUser {
	/// The old state
	pub old: User,
	/// The new state
	pub new: User,
}

/// LDAP-specific configuration
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SourceLdapConfig {
	/// The URL of the LDAP/AD server
	pub url: Url,
	/// The base DN for searching users
	pub base_dn: String,
	/// The DN to bind for authentication
	pub bind_dn: String,
	/// The password for the bind DN
	pub bind_password: String,
	/// Filter to apply when searching for users, e.g., (objectClass=person) DO
	/// NOT FILTER STATUS!
	pub user_filter: String,
	/// Timeout for LDAP operations in seconds
	pub timeout: u64,
	/// A mapping from the mostly free-form LDAP attributes to
	/// attribute names as used by famedly
	pub attributes: LdapAttributesMapping,
	/// Whether to update deleted entries
	pub check_for_deleted_entries: bool,
	/// Whether to ask LDAP for specific attributes or just specify *.
	/// Various implementations either do or don't send data in both
	/// cases, so this needs to be tested against the actual server.
	pub use_attribute_filter: bool,
	/// TLS-related configuration
	pub tls: Option<LdapTlsConfig>,
}

impl From<SourceLdapConfig> for ldap_poller::Config {
	fn from(cfg: SourceLdapConfig) -> ldap_poller::Config {
		let starttls = cfg.tls.as_ref().is_some_and(|tls| tls.danger_use_start_tls);
		let no_tls_verify = cfg.tls.as_ref().is_some_and(|tls| tls.danger_disable_tls_verify);
		let root_certificates_path =
			cfg.tls.as_ref().and_then(|tls| tls.server_certificate.clone());
		let client_key_path = cfg.tls.as_ref().and_then(|tls| tls.client_key.clone());
		let client_certificate_path =
			cfg.tls.as_ref().and_then(|tls| tls.client_certificate.clone());

		let tls = TLSConfig {
			starttls,
			no_tls_verify,
			root_certificates_path,
			client_key_path,
			client_certificate_path,
		};

		let attributes = cfg.attributes;
		ldap_poller::Config {
			url: cfg.url,
			connection: ConnectionConfig {
				timeout: cfg.timeout,
				operation_timeout: std::time::Duration::from_secs(cfg.timeout),
				tls,
			},
			search_user: cfg.bind_dn,
			search_password: cfg.bind_password,
			searches: Searches {
				user_base: cfg.base_dn,
				user_filter: cfg.user_filter,
				page_size: None,
			},
			attributes: AttributeConfig {
				pid: attributes.user_id.get_name(),
				updated: attributes.last_modified.map(AttributeMapping::get_name),
				additional: vec![],
				filter_attributes: cfg.use_attribute_filter,
				attrs_to_track: vec![
					attributes.status.get_name(),
					attributes.first_name.get_name(),
					attributes.last_name.get_name(),
					attributes.preferred_username.get_name(),
					attributes.email.get_name(),
					attributes.phone.get_name(),
				],
			},
			cache_method: CacheMethod::ModificationTime,
			check_for_deleted_entries: cfg.check_for_deleted_entries,
		}
	}
}

/// A mapping from the mostly free-form LDAP attributes to attribute
/// names as used by famedly
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct LdapAttributesMapping {
	/// Attribute for the user's first name
	pub first_name: AttributeMapping,
	/// Attribute for the user's last name
	pub last_name: AttributeMapping,
	/// Attribute for the user's preferred username
	pub preferred_username: AttributeMapping,
	/// Attribute for the user's email address
	pub email: AttributeMapping,
	/// Attribute for the user's phone number
	pub phone: AttributeMapping,
	/// Attribute for the user's unique ID
	pub user_id: AttributeMapping,
	/// This attribute shows the account status (It expects an i32 like
	/// userAccountControl in AD)
	pub status: AttributeMapping,
	/// Marks an account as disabled (for example userAccountControl: bit flag
	/// ACCOUNTDISABLE would be 2)
	#[serde(default)]
	pub disable_bitmasks: Vec<i32>,
	/// Last modified
	pub last_modified: Option<AttributeMapping>,
}

/// How an attribute should be defined in config - it can either be a
/// raw string, *or* it can be a struct defining both an attribute
/// name and whether the attribute should be treated as binary.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum AttributeMapping {
	/// An attribute that's defined without specifying whether it is
	/// binary or not
	NoBinaryOption(String),
	/// An attribute that specifies whether it is binary or not
	OptionalBinary {
		/// The name of the attribute
		name: String,
		/// Whether the attribute is binary
		#[serde(default)]
		is_binary: bool,
	},
}

impl AttributeMapping {
	/// Get the attribute name
	#[must_use]
	pub fn get_name(self) -> String {
		match self {
			Self::NoBinaryOption(name) => name,
			Self::OptionalBinary { name, .. } => name,
		}
	}
}

impl Display for AttributeMapping {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.clone().get_name())
	}
}

/// The LDAP TLS configuration
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct LdapTlsConfig {
	/// Path to the client key; if not specified, it will be assumed
	/// that the server is configured not to verify client
	/// certificates.
	pub client_key: Option<PathBuf>,
	/// Path to the client certificate; if not specified, it will be
	/// assumed that the server is configured not to verify client
	/// certificates.
	pub client_certificate: Option<PathBuf>,
	/// Path to the server certificate; if not specified, the host's
	/// CA will be used to verify the server.
	pub server_certificate: Option<PathBuf>,
	/// Whether to verify the server's certificates.
	///
	/// This should normally only be used in test environments, as
	/// disabling certificate validation defies the purpose of using
	/// TLS in the first place.
	#[serde(default)]
	pub danger_disable_tls_verify: bool,
	/// Enable StartTLS, i.e., use the non-TLS ldap port, but send a
	/// special message to upgrade the connection to TLS.
	///
	/// This is less secure than standard TLS, an `ldaps` URL should
	/// be preferred.
	#[serde(default)]
	pub danger_use_start_tls: bool,
}
