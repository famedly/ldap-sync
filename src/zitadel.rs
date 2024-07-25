//! Helper functions for submitting data to Zitadel
use anyhow::{anyhow, Result};
use itertools::Itertools;
use ldap_poller::ldap3::SearchEntry;
use zitadel_rust_client::{
	Email, Gender, Idp, ImportHumanUserRequest, Phone, Profile, Zitadel as ZitadelClient,
};

use crate::config::{Config, FeatureFlag};

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
				nick_name: String::default(),
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
