//! User data helpers
use std::fmt::Display;

use base64::prelude::{Engine, BASE64_STANDARD};
use zitadel_rust_client::{Email, Gender, Idp, ImportHumanUserRequest, Phone, Profile};

/// Crate-internal representation of a Zitadel/LDAP user
#[derive(Clone, Debug)]
pub(crate) struct User {
	/// The user's first name
	pub(crate) first_name: StringOrBytes,
	/// The user's last name
	pub(crate) last_name: StringOrBytes,
	/// The user's preferred username
	pub(crate) preferred_username: StringOrBytes,
	/// The user's email address
	pub(crate) email: StringOrBytes,
	/// The user's LDAP ID
	pub(crate) ldap_id: StringOrBytes,
	/// The user's phone number
	pub(crate) phone: Option<StringOrBytes>,
	/// Whether the user is enabled
	pub(crate) enabled: bool,

	/// Whether the user should be prompted to verify their email
	pub(crate) needs_email_verification: bool,
	/// Whether the user should be prompted to verify their phone number
	pub(crate) needs_phone_verification: bool,
	/// The ID of the identity provider to link with, if any
	pub(crate) idp_id: Option<String>,
}

impl User {
	/// Get a display name for the user
	pub(crate) fn get_display_name(&self) -> String {
		format!("{}, {}", self.last_name, self.first_name)
	}

	/// Return the name to be used in logs to identify this user
	pub(crate) fn log_name(&self) -> String {
		format!("email={}", &self.email)
	}

	/// Get idp link as required by Zitadel
	fn get_idps(&self) -> Vec<Idp> {
		if let Some(idp_id) = self.idp_id.clone() {
			vec![Idp {
				config_id: idp_id,
				external_user_id: self.ldap_id.clone().to_string(),
				display_name: self.get_display_name(),
			}]
		} else {
			vec![]
		}
	}
}

impl From<User> for ImportHumanUserRequest {
	fn from(user: User) -> Self {
		Self {
			user_name: user.email.clone().to_string(),
			profile: Some(Profile {
				first_name: user.first_name.clone().to_string(),
				last_name: user.last_name.clone().to_string(),
				display_name: user.get_display_name(),
				gender: Gender::Unspecified.into(), // 0 means "unspecified",
				nick_name: user.ldap_id.clone().to_string(),
				preferred_language: String::default(),
			}),
			email: Some(Email {
				email: user.email.clone().to_string(),
				is_email_verified: !user.needs_email_verification,
			}),
			phone: user.phone.as_ref().map(|phone| Phone {
				phone: phone.to_owned().to_string(),
				is_phone_verified: !user.needs_phone_verification,
			}),
			password: String::default(),
			hashed_password: None,
			password_change_required: false,
			request_passwordless_registration: false,
			otp_code: String::default(),
			idps: user.get_idps(),
		}
	}
}

impl Display for User {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "email={}", &self.email)
	}
}

/// A structure that can either be a string or bytes
#[derive(Clone, Debug)]
pub(crate) enum StringOrBytes {
	/// A string
	String(String),
	/// A byte string
	Bytes(Vec<u8>),
}

impl PartialEq for StringOrBytes {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::String(s), Self::String(o)) => s == o,
			(Self::String(s), Self::Bytes(o)) => s.as_bytes() == o,
			(Self::Bytes(s), Self::String(o)) => s == o.as_bytes(),
			(Self::Bytes(s), Self::Bytes(o)) => s == o,
		}
	}
}

impl Display for StringOrBytes {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			StringOrBytes::String(value) => write!(f, "{}", value),
			StringOrBytes::Bytes(value) => write!(f, "{}", BASE64_STANDARD.encode(value)),
		}
	}
}

impl From<String> for StringOrBytes {
	fn from(value: String) -> Self {
		Self::String(value)
	}
}
