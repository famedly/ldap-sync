//! Helper functions for submitting data to Zitadel
use anyhow::{bail, Context, Result};
use uuid::{uuid, Uuid};
use zitadel_rust_client::{
	error::{Error as ZitadelError, TonicErrorCode},
	Zitadel as ZitadelClient,
};

use crate::{
	config::{Config, FeatureFlags, ZitadelConfig},
	user::{StringOrBytes, User},
};

/// The Famedly UUID namespace to use to generate v5 UUIDs.
const FAMEDLY_NAMESPACE: Uuid = uuid!("d9979cff-abee-4666-bc88-1ec45a843fb8");

/// The Zitadel project role to assign to users.
const FAMEDLY_USER_ROLE: &str = "User";

/// A very high-level Zitadel zitadel_client
#[derive(Clone)]
pub(crate) struct Zitadel {
	/// ldap-sync configuration
	zitadel_config: ZitadelConfig,
	/// Optional set of features
	feature_flags: FeatureFlags,
	/// The backing Zitadel zitadel_client
	zitadel_client: ZitadelClient,
}

impl Zitadel {
	/// Construct the Zitadel instance
	pub(crate) async fn new(config: &Config) -> Result<Self> {
		let zitadel_client = ZitadelClient::new(
			config.zitadel_config.url.clone(),
			config.zitadel_config.key_file.clone(),
		)
		.await
		.context("failed to configure zitadel_client")?;

		Ok(Self {
			zitadel_config: config.zitadel_config.clone(),
			feature_flags: config.feature_flags.clone(),
			zitadel_client,
		})
	}
	/// Import a list of new users into Zitadel
	pub(crate) async fn import_new_users(&self, users: Vec<User>) -> Result<()> {
		for user in users {
			let sync_status = self.import_user(&user).await;

			if let Err(error) = sync_status {
				tracing::error!("Failed to sync user `{}`: {}", user.log_name(), error);
			};
		}

		Ok(())
	}

	/// Delete a list of Zitadel users given their IDs
	pub(crate) async fn delete_users_by_id(&self, users: Vec<String>) -> Result<()> {
		for user_id in users {
			let status = self.delete_user_by_id(&user_id).await;

			if let Err(error) = status {
				tracing::error!("Failed to delete user `{}`: {}", user_id, error);
			}
		}

		Ok(())
	}

	/// Delete a list of Zitadel users given their Email Addresses
	pub(crate) async fn delete_users_by_email(&self, users: Vec<String>) -> Result<()> {
		for email in users {
			let status = self.delete_user_by_email(&email).await;

			if let Err(error) = status {
				tracing::error!("Failed to delete user `{}`: {}", email, error);
			}
		}

		Ok(())
	}

	/// Update a list of old/new user maps
	pub(crate) async fn update_users(&self, users: Vec<(User, User)>) -> Result<()> {
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

		if !self.feature_flags.deactivate_only() {
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
		}

		Ok(())
	}

	/// Update a Zitadel user
	#[allow(clippy::unused_async, unused_variables)]
	async fn update_user(&self, old: &User, new: &User) -> Result<()> {
		if self.feature_flags.dry_run() {
			tracing::info!("Not updating user due to dry run: {:?} -> {:?}", old, new);
			return Ok(());
		}

		let Some(user_id) = self.get_user_id(old).await? else {
			bail!("could not find user `{}` to update", old.email);
		};

		if old.email != new.email {
			self.zitadel_client
				.update_human_user_name(
					&self.zitadel_config.organization_id,
					user_id.clone(),
					new.email.clone().to_string(),
				)
				.await?;

			tracing::warn!("User email/login changed for {} -> {}", old.email, new.email);
		};

		if old.first_name != new.first_name || old.last_name != new.last_name {
			self.zitadel_client
				.update_human_user_profile(
					&self.zitadel_config.organization_id,
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
				self.zitadel_client
					.remove_human_user_phone(&self.zitadel_config.organization_id, user_id.clone())
					.await?;
			}
			(_, Some(new_phone)) => {
				self.zitadel_client
					.update_human_user_phone(
						&self.zitadel_config.organization_id,
						user_id.clone(),
						new_phone.clone().to_string(),
						!self.feature_flags.require_phone_verification(),
					)
					.await?;
			}
			(None, None) => {}
		};

		if old.email != new.email {
			self.zitadel_client
				.update_human_user_email(
					&self.zitadel_config.organization_id,
					user_id.clone(),
					new.email.clone().to_string(),
					!self.feature_flags.require_email_verification(),
				)
				.await?;
		};

		if old.preferred_username != new.preferred_username {
			self.zitadel_client
				.set_user_metadata(
					Some(&self.zitadel_config.organization_id),
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
	async fn delete_user_by_id(&self, user_id: &str) -> Result<()> {
		if self.feature_flags.dry_run() {
			tracing::info!("Not deleting user `{}` due to dry run", user_id);
			return Ok(());
		}

		let user = self
			.zitadel_client
			.get_user_by_nick_name(
				Some(self.zitadel_config.organization_id.clone()),
				user_id.to_owned(),
			)
			.await?;
		match user {
			Some(user) => self.zitadel_client.remove_user(user.id).await?,
			None => bail!("Could not find user with ldap uid '{user_id}' for deletion"),
		}

		tracing::info!("Successfully deleted user {}", user_id);

		Ok(())
	}

	/// Delete a Zitadel user given only their email address
	async fn delete_user_by_email(&self, email: &str) -> Result<()> {
		if self.feature_flags.dry_run() {
			tracing::info!("Not deleting user `{}` due to dry run", email);
			return Ok(());
		}

		// TODO: Implement delete_user_by_email in zitadel_rust_client

		/*
			let user = self
				.zitadel_client
				.get_user_by_email(Some(self.config.zitadel.organization_id.clone()), email.to_owned())
				.await?;
			match user {
				Some(user) => self.zitadel_client.remove_user(user.id).await?,
				None => bail!("Could not find user with email '{email}' for deletion"),
			}

			tracing::info!("Successfully deleted user {}", email);
		*/

		Ok(())
	}

	/// Retrieve the Zitadel user ID of a user, or None if the user
	/// cannot be found
	async fn get_user_id(&self, user: &User) -> Result<Option<String>> {
		let status =
			self.zitadel_client.get_user_by_login_name(&user.email.clone().to_string()).await;

		if let Err(ZitadelError::TonicResponseError(ref error)) = status {
			if error.code() == TonicErrorCode::NotFound {
				return Ok(None);
			}
		}

		Ok(status.map(|user| user.map(|u| u.id))?)
	}

	/// Delete a Zitadel user
	async fn delete_user(&self, user: &User) -> Result<()> {
		if self.feature_flags.dry_run() {
			tracing::info!("Not deleting user due to dry run: {:?}", user);
			return Ok(());
		}

		if let Some(user_id) = self.get_user_id(user).await? {
			self.zitadel_client.remove_user(user_id).await?;
		} else {
			bail!("could not find user `{}` for deletion", user.email);
		}

		tracing::info!("Successfully deleted user {}", user.email);

		Ok(())
	}

	/// Import a user into Zitadel
	async fn import_user(&self, user: &User) -> Result<()> {
		if self.feature_flags.dry_run() {
			tracing::info!("Not importing user due to dry run: {:?}", user);
			return Ok(());
		}

		let new_user_id = self
			.zitadel_client
			.create_human_user(&self.zitadel_config.organization_id, user.clone().into())
			.await?;

		self.zitadel_client
			.set_user_metadata(
				Some(&self.zitadel_config.organization_id),
				new_user_id.clone(),
				"preferred_username".to_owned(),
				&user.preferred_username.clone().to_string(),
			)
			.await?;

		let id = match &user.ldap_id {
			StringOrBytes::String(value) => value.as_bytes(),
			StringOrBytes::Bytes(value) => value,
		};

		self.zitadel_client
			.set_user_metadata(
				Some(&self.zitadel_config.organization_id),
				new_user_id.clone(),
				"localpart".to_owned(),
				&Uuid::new_v5(&FAMEDLY_NAMESPACE, id).to_string(),
			)
			.await?;

		self.zitadel_client
			.add_user_grant(
				Some(self.zitadel_config.organization_id.clone()),
				new_user_id,
				self.zitadel_config.project_id.clone(),
				None,
				vec![FAMEDLY_USER_ROLE.to_owned()],
			)
			.await?;

		tracing::info!("Successfully imported user {}", user.email);

		Ok(())
	}
}
