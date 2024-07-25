//! Helper functions for submitting data to Zitadel
use anyhow::{anyhow, bail, Result};
use itertools::Itertools;
use ldap_poller::ldap3::SearchEntry;
use uuid::{uuid, Uuid};
use zitadel_rust_client::{
	Email, Gender, Idp, ImportHumanUserRequest, Phone, Profile, Zitadel as ZitadelClient,
};

use crate::config::{Config, FeatureFlag};

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
				.map_err(|message| anyhow!("failed to configure zitadel client: {}", message))?;

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
				tracing::error!("Failed to sync user `{}`: {}", user.ldap_id, error);
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
				tracing::error!("Failed to delete user `{}`: {}`", user.ldap_id, error);
			}
		}

		for user in enabled {
			let status = self.import_user(&user).await;

			if let Err(error) = status {
				tracing::error!("Failed to re-create user `{}`: {}", user.ldap_id, error);
			}
		}

		for (old, new) in changed {
			let status = self.update_user(&old, &new).await;

			if let Err(error) = status {
				tracing::error!("Failed to update user `{}`: {}", new.ldap_id, error);
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
		todo!()
	}

	/// Delete a Zitadel user given only their LDAP id
	async fn delete_user_by_id(&self, ldap_id: &[u8]) -> Result<()> {
		let uid = String::from_utf8(ldap_id.to_vec())?;
		let user = self
			.client
			.get_user_by_nick_name(Some(self.config.famedly.organization_id.clone()), uid.clone())
			.await?;
		match user {
			Some(user) => self.client.remove_user(user.id).await?,
			None => bail!("Could not find user with ldap uid '{uid}' for deletion"),
		}

		Ok(())
	}

	/// Delete a Zitadel user
	async fn delete_user(&self, user: &User) -> Result<()> {
		if let Some(user_id) = self.client.get_user_by_login_name(&user.email).await? {
			self.client.remove_user(user_id.id).await?;
		} else {
			bail!("could not find user `{}` for deletion", user.email);
		}
		Ok(())
	}

	/// Import a user into Zitadel
	async fn import_user(&self, user: &User) -> Result<()> {
		let new_user_id = self
			.client
			.create_human_user(&self.config.famedly.organization_id, user.clone().into())
			.await?;

		self.client
			.set_user_metadata(
				Some(&self.config.famedly.organization_id),
				new_user_id.clone(),
				"preferred_username".to_owned(),
				&user.preferred_username,
			)
			.await?;

		self.client
			.set_user_metadata(
				Some(&self.config.famedly.organization_id),
				new_user_id.clone(),
				"localpart".to_owned(),
				&Uuid::new_v5(&FAMEDLY_NAMESPACE, user.ldap_id.as_bytes()).to_string(),
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

		Ok(())
	}
}

/// Crate-internal representation of a Zitadel/LDAP user
#[derive(Clone)]
struct User {
	/// The user's first name
	first_name: String,
	/// The user's last name
	last_name: String,
	/// The user's preferred username
	preferred_username: String,
	/// The user's email address
	email: String,
	/// The user's LDAP ID
	ldap_id: String,
	/// The user's phone number
	phone: String,
	/// Whether the user is enabled
	enabled: bool,

	/// Whether the user should be prompted to verify their email
	needs_email_verification: bool,
	/// Whether the user should be prompted to verify their phone number
	needs_phone_verification: bool,
	/// Identity providers to link the user with, if any
	idps: Vec<Idp>,
}

impl User {
	/// Construct a user from an LDAP SearchEntry
	fn try_from_search_entry(entry: SearchEntry, config: &Config) -> Result<Self> {
		/// Read an attribute from the entry
		fn read_entry(entry: &SearchEntry, attribute: &str) -> Result<String> {
			entry
				.attrs
				.get(attribute)
				.ok_or(anyhow!("missing attribute `{}` for `{}`", attribute, entry.dn))
				.and_then(|values| {
					values.first().ok_or(anyhow!(
						"missing `{}` values for `{}`",
						attribute,
						entry.dn
					))
				})
				.cloned()
		}

		let enabled = read_entry(&entry, &config.ldap.attributes.status)?
			!= config.ldap.attributes.disable_value;
		let first_name = read_entry(&entry, &config.ldap.attributes.first_name)?;
		let last_name = read_entry(&entry, &config.ldap.attributes.last_name)?;
		let preferred_username = read_entry(&entry, &config.ldap.attributes.preferred_username)?;
		let email = read_entry(&entry, &config.ldap.attributes.email)?;
		let user_id = read_entry(&entry, &config.ldap.attributes.user_id)?;
		let phone = read_entry(&entry, &config.ldap.attributes.phone)?;

		let display_name = format!("{last_name}, {first_name}");

		let idps = if config.feature_flags.contains(&FeatureFlag::SsoLogin) {
			vec![Idp {
				config_id: config.famedly.idp_id.clone(),
				external_user_id: user_id.clone(),
				display_name: display_name.clone(),
			}]
		} else {
			vec![]
		};

		Ok(Self {
			first_name,
			last_name,
			preferred_username,
			email,
			ldap_id: user_id,
			phone,
			enabled,
			needs_email_verification: config.feature_flags.contains(&FeatureFlag::VerifyEmail),
			needs_phone_verification: config.feature_flags.contains(&FeatureFlag::VerifyPhone),
			idps,
		})
	}
}

impl From<User> for ImportHumanUserRequest {
	fn from(user: User) -> Self {
		Self {
			user_name: user.email.clone(),
			profile: Some(Profile {
				first_name: user.first_name.clone(),
				last_name: user.last_name.clone(),
				display_name: format!("{}, {}", user.last_name, user.first_name),
				gender: Gender::Unspecified.into(), // 0 means "unspecified",
				nick_name: user.ldap_id.clone(),
				preferred_language: String::default(),
			}),
			email: Some(Email {
				email: user.email,
				is_email_verified: !user.needs_email_verification,
			}),
			phone: Some(Phone {
				phone: user.phone,
				is_phone_verified: !user.needs_phone_verification,
			}),
			password: String::default(),
			hashed_password: None,
			password_change_required: false,
			request_passwordless_registration: true,
			otp_code: String::default(),
			idps: user.idps,
		}
	}
}
