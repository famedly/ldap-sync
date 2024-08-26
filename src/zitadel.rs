//! Helper functions for submitting data to Zitadel
use anyhow::{bail, Context, Result};
use itertools::Itertools;
use ldap_poller::ldap3::SearchEntry;
use uuid::{uuid, Uuid};
use zitadel_rust_client::{
	error::{Error as ZitadelError, TonicErrorCode},
	Zitadel as ZitadelClient,
};

use crate::{
	config::Config,
	user::{StringOrBytes, User},
};

/// The Famedly UUID namespace to use to generate v5 UUIDs.
const FAMEDLY_NAMESPACE: Uuid = uuid!("d9979cff-abee-4666-bc88-1ec45a843fb8");

/// The Zitadel project role to assign to users.
const FAMEDLY_USER_ROLE: &str = "User";

/// A very high-level Zitadel client
pub(crate) struct Zitadel {
	/// The backing Zitadel client
	client: ZitadelClient,
	/// ldap-sync configuration
	config: Config,
}

impl Zitadel {
	/// Construct the Zitadel instance
	pub(crate) async fn new(config: &Config) -> Result<Self> {
		let client =
			ZitadelClient::new(config.famedly.url.clone(), config.famedly.key_file.clone())
				.await
				.context("failed to configure zitadel client")?;

		Ok(Self { client, config: config.clone() })
	}

	/// Import a list of new users into Zitadel
	pub(crate) async fn import_new_users(&self, users: Vec<SearchEntry>) -> Result<()> {
		let (users, invalid): (Vec<_>, Vec<_>) = users
			.into_iter()
			.filter_map(|user| {
				User::try_from_search_entry(user, &self.config)
					.map(|user| user.enabled.then_some(user))
					.transpose()
			})
			.partition_result();

		if !invalid.is_empty() {
			let messages = invalid
				.into_iter()
				.fold(String::default(), |acc, error| acc + error.to_string().as_str() + "\n");

			tracing::warn!("Some users cannot be synced due to missing attributes:\n{}", messages);
		}

		for user in users {
			let sync_status = self.import_user(&user).await;

			if let Err(error) = sync_status {
				tracing::error!("Failed to sync user `{}`: {}", user.log_name(), error);
			};
		}

		Ok(())
	}

	/// Update a list of old/new user maps
	pub(crate) async fn update_users(&self, users: Vec<(SearchEntry, SearchEntry)>) -> Result<()> {
		let (users, invalid): (Vec<_>, Vec<anyhow::Error>) = users
			.into_iter()
			.map(|(old, new)| {
				let old = User::try_from_search_entry(old, &self.config)?;
				let new = User::try_from_search_entry(new, &self.config)?;

				Ok((old, new))
			})
			.partition_result();

		if !invalid.is_empty() {
			let messages = invalid
				.into_iter()
				.fold(String::default(), |acc, error| acc + error.to_string().as_str() + "\n");

			tracing::warn!("Some users cannot be updated due to missing attributes:\n{}", messages);
		}

		let disabled: Vec<User> = users
			.iter()
			.filter(|&(old, new)| old.enabled && !new.enabled)
			.map(|(_, new)| new.clone())
			.collect();

		let enabled: Vec<User> = users
			.iter()
			.filter(|(old, new)| !old.enabled && new.enabled)
			.map(|(_, new)| new.clone())
			.collect();

		let changed: Vec<(User, User)> = users
			.into_iter()
			.filter(|(old, new)| new.enabled && old.enabled == new.enabled)
			.collect();

		for user in disabled {
			let status = self.delete_user(&user).await;

			if let Err(error) = status {
				tracing::error!("Failed to delete user `{}`: {}`", user.log_name(), error);
			}
		}

		for user in enabled {
			let status = self.import_user(&user).await;

			if let Err(error) = status {
				tracing::error!("Failed to re-create user `{}`: {}", user.log_name(), error);
			}
		}

		for (old, new) in changed {
			let status = self.update_user(&old, &new).await;

			if let Err(error) = status {
				tracing::error!("Failed to update user `{}`: {}", new.log_name(), error);
			}
		}

		Ok(())
	}

	/// Delete a list of Zitadel users given their IDs
	pub(crate) async fn delete_users(&self, users: Vec<Vec<u8>>) -> Result<()> {
		for user in users {
			let status = self.delete_user_by_id(&user).await;

			if let Err(error) = status {
				// This is only used for logging, so if the string is
				// invalid it should be fine
				let user_id = String::from_utf8_lossy(&user);

				tracing::error!("Failed to delete user `{}`: {}", user_id, error);
			}
		}

		Ok(())
	}

	/// Update a Zitadel user
	#[allow(clippy::unused_async, unused_variables)]
	async fn update_user(&self, old: &User, new: &User) -> Result<()> {
		if self.config.dry_run() {
			tracing::info!("Not updating user due to dry run: {:?} -> {:?}", old, new);
			return Ok(());
		}

		let Some(user_id) = self.get_user_id(old).await? else {
			bail!("could not find user `{}` to update", old.email);
		};

		if old.email != new.email {
			self.client
				.update_human_user_name(
					&self.config.famedly.organization_id,
					user_id.clone(),
					new.email.clone().to_string(),
				)
				.await?;

			tracing::warn!("User email/login changed for {} -> {}", old.email, new.email);
		};

		if old.first_name != new.first_name || old.last_name != new.last_name {
			self.client
				.update_human_user_profile(
					&self.config.famedly.organization_id,
					user_id.clone(),
					new.first_name.clone().to_string(),
					new.last_name.clone().to_string(),
					None,
					Some(new.get_display_name()),
					None,
					None,
				)
				.await?;
		};

		match (&old.phone, &new.phone) {
			(Some(_), None) => {
				self.client
					.remove_human_user_phone(&self.config.famedly.organization_id, user_id.clone())
					.await?;
			}
			(_, Some(new_phone)) => {
				self.client
					.update_human_user_phone(
						&self.config.famedly.organization_id,
						user_id.clone(),
						new_phone.clone().to_string(),
						!self.config.require_phone_verification(),
					)
					.await?;
			}
			(None, None) => {}
		};

		if old.email != new.email {
			self.client
				.update_human_user_email(
					&self.config.famedly.organization_id,
					user_id.clone(),
					new.email.clone().to_string(),
					!self.config.require_email_verification(),
				)
				.await?;
		};

		if old.preferred_username != new.preferred_username {
			self.client
				.set_user_metadata(
					Some(&self.config.famedly.organization_id),
					user_id,
					"preferred_username".to_owned(),
					&new.preferred_username.clone().to_string(),
				)
				.await?;
		};

		tracing::info!("Successfully updated user {}", old.email);

		Ok(())
	}

	/// Delete a Zitadel user given only their LDAP id
	async fn delete_user_by_id(&self, ldap_id: &[u8]) -> Result<()> {
		if self.config.dry_run() {
			tracing::info!(
				"Not deleting user `{}` due to dry run",
				String::from_utf8_lossy(ldap_id)
			);
			return Ok(());
		}

		let uid = String::from_utf8(ldap_id.to_vec())?;
		let user = self
			.client
			.get_user_by_nick_name(Some(self.config.famedly.organization_id.clone()), uid.clone())
			.await?;
		match user {
			Some(user) => self.client.remove_user(user.id).await?,
			None => bail!("Could not find user with ldap uid '{uid}' for deletion"),
		}

		tracing::info!("Successfully deleted user {}", String::from_utf8_lossy(ldap_id));

		Ok(())
	}

	/// Retrieve the Zitadel user ID of a user, or None if the user
	/// cannot be found
	async fn get_user_id(&self, user: &User) -> Result<Option<String>> {
		let status = self.client.get_user_by_login_name(&user.email.clone().to_string()).await;

		if let Err(ZitadelError::TonicResponseError(ref error)) = status {
			if error.code() == TonicErrorCode::NotFound {
				return Ok(None);
			}
		}

		Ok(status.map(|user| user.map(|u| u.id))?)
	}

	/// Delete a Zitadel user
	async fn delete_user(&self, user: &User) -> Result<()> {
		if self.config.dry_run() {
			tracing::info!("Not deleting user due to dry run: {:?}", user);
			return Ok(());
		}

		if let Some(user_id) = self.get_user_id(user).await? {
			self.client.remove_user(user_id).await?;
		} else {
			bail!("could not find user `{}` for deletion", user.email);
		}

		tracing::info!("Successfully deleted user {}", user.email);

		Ok(())
	}

	/// Import a user into Zitadel
	async fn import_user(&self, user: &User) -> Result<()> {
		if self.config.dry_run() {
			tracing::info!("Not importing user due to dry run: {:?}", user);
			return Ok(());
		}

		let new_user_id = self
			.client
			.create_human_user(&self.config.famedly.organization_id, user.clone().into())
			.await?;

		self.client
			.set_user_metadata(
				Some(&self.config.famedly.organization_id),
				new_user_id.clone(),
				"preferred_username".to_owned(),
				&user.preferred_username.clone().to_string(),
			)
			.await?;

		let id = match &user.ldap_id {
			StringOrBytes::String(value) => value.as_bytes(),
			StringOrBytes::Bytes(value) => value,
		};

		self.client
			.set_user_metadata(
				Some(&self.config.famedly.organization_id),
				new_user_id.clone(),
				"localpart".to_owned(),
				&Uuid::new_v5(&FAMEDLY_NAMESPACE, id).to_string(),
			)
			.await?;

		self.client
			.add_user_grant(
				Some(self.config.famedly.organization_id.clone()),
				new_user_id,
				self.config.famedly.project_id.clone(),
				None,
				vec![FAMEDLY_USER_ROLE.to_owned()],
			)
			.await?;

		tracing::info!("Successfully imported user {}", user.email);

		Ok(())
	}
}
