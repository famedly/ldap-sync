//! Helper functions for submitting data to Zitadel
use anyhow::{anyhow, Result};
use ldap_poller::ldap3::SearchEntry;
use zitadel_rust_client::{
	Config as ZitadelConfig, Email, Gender, Idp, ImportHumanUserRequest, Phone, Profile,
	Zitadel as ZitadelClient,
};

use crate::config::{Config, FeatureFlag};

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
		let zitadel_config =
			ZitadelConfig::new(config.clone().famedly.url, config.clone().famedly.key_file);

		let client = ZitadelClient::new(&zitadel_config)
			.await
			.map_err(|message| anyhow!("failed to configure zitadel client: {}", message))?;

		Ok(Self { client, config: config.clone() })
	}

	/// Import a list of new users into Zitadel
	pub(crate) async fn import_new_users(&self, users: Vec<SearchEntry>) -> Result<()> {
		let (users, invalid): (
			Vec<Result<ImportHumanUserRequest>>,
			Vec<Result<ImportHumanUserRequest>>,
		) = users
			.into_iter()
			.map(|user| user_from_ldap(&user, &self.config))
			.partition(Result::is_ok);

		if !invalid.is_empty() {
			let messages = invalid
				.into_iter()
				.filter_map(std::result::Result::err)
				.fold(String::default(), |acc, error| acc + error.to_string().as_str() + "\n");

			tracing::warn!("Some users cannot be synced due to missing attributes:\n{}", messages);
		}

		for user in users {
			if let Ok(user) = user {
				self.client.create_human_user(&self.config.famedly.organization_id, user).await?;
			} else {
				tracing::error!(
					"Hit error in converted user, this should not be possible: {:?}",
					user
				);
			}
		}

		Ok(())
	}
}

/// An error arising from a conversion from an ldap search entry to an
/// ImportHumanUserRequest.
///
/// Convert an ldap entry into a user import request
fn user_from_ldap(entry: &SearchEntry, config: &Config) -> Result<ImportHumanUserRequest> {
	/// Read an attribute from the entry
	fn read_entry(entry: &SearchEntry, attribute: &str) -> Result<String> {
		entry
			.attrs
			.get(attribute)
			.ok_or(anyhow!("missing attribute `{}` for `{}`", attribute, entry.dn))
			.and_then(|values| {
				values.first().ok_or(anyhow!("missing `{}` values for `{}`", attribute, entry.dn))
			})
			.cloned()
	}

	let first_name = read_entry(entry, &config.ldap.attributes.first_name)?;
	let last_name = read_entry(entry, &config.ldap.attributes.last_name)?;
	let _preferred_username = read_entry(entry, &config.ldap.attributes.preferred_username)?;
	let email = read_entry(entry, &config.ldap.attributes.email)?;
	let user_id = read_entry(entry, &config.ldap.attributes.user_id)?;
	let phone = read_entry(entry, &config.ldap.attributes.phone)?;
	let _status = read_entry(entry, &config.ldap.attributes.status)?;

	let display_name = format!("{last_name}, {first_name}");

	let idps = if config.feature_flags.contains(&FeatureFlag::SsoLogin) {
		vec![Idp {
			config_id: config.famedly.idp_id.clone(),
			external_user_id: user_id,
			display_name: display_name.clone(),
		}]
	} else {
		vec![]
	};

	Ok(ImportHumanUserRequest {
		user_name: email.clone(),
		profile: Some(Profile {
			first_name,
			last_name,
			display_name,
			gender: Gender::Unspecified.into(), // 0 means "unspecified",
			nick_name: String::default(),
			preferred_language: String::default(),
		}),
		email: Some(Email {
			email,
			is_email_verified: !config.feature_flags.contains(&FeatureFlag::VerifyEmail),
		}),
		phone: Some(Phone {
			phone,
			is_phone_verified: !config.feature_flags.contains(&FeatureFlag::VerifyPhone),
		}),
		password: String::default(),
		hashed_password: None,
		password_change_required: false,
		request_passwordless_registration: true,
		otp_code: String::default(),
		idps,
	})
}
