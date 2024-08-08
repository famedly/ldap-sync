//! All sync client configuration structs and logic
use std::{
	fmt::Display,
	path::{Path, PathBuf},
};

use anyhow::{bail, Result};
use ldap_poller::{config::TLSConfig, AttributeConfig, CacheMethod, ConnectionConfig, Searches};
use serde::Deserialize;
use url::Url;

impl Config {
	/// Read the config from a file
	pub async fn from_file(path: &Path) -> Result<Self> {
		let config: Config = serde_yaml::from_slice(&tokio::fs::read(path).await?)?;
		config.validate()
	}

	/// Validate the config and return a valid configuration
	fn validate(mut self) -> Result<Self> {
		self.famedly.url = validate_famedly_url(self.famedly.url)?;

		Ok(self)
	}

	/// Whether phone verification is enabled
	#[must_use]
	pub fn require_phone_verification(&self) -> bool {
		self.feature_flags.contains(&FeatureFlag::VerifyPhone)
	}

	/// Whether email verification is enabled
	#[must_use]
	pub fn require_email_verification(&self) -> bool {
		self.feature_flags.contains(&FeatureFlag::VerifyEmail)
	}

	/// Whether SSO login is enabled
	#[must_use]
	pub fn require_sso_login(&self) -> bool {
		self.feature_flags.contains(&FeatureFlag::SsoLogin)
	}
}

/// Validate the famedly URL
fn validate_famedly_url(url: Url) -> Result<Url> {
	// If a URL contains a port, the domain name may appear as a
	// scheme and pass through URL parsing despite lacking a scheme
	if url.scheme() != "https" && url.scheme() != "http" {
		bail!("famedly URL scheme must be `http` or `https`, e.g. `https://{}`", url);
	}

	Ok(url)
}

#[cfg(test)]
mod tests {
	#![allow(clippy::expect_used)]
	use super::*;

	#[test]
	fn test_famedly_url_validate_valid() {
		let url = Url::parse("https://famedly.de").expect("invalid url");
		let validated = validate_famedly_url(url).expect("url failed to validate");
		assert_eq!(validated.to_string(), "https://famedly.de/");
	}

	#[test]
	fn test_famedly_url_validate_trailing_slash_path() {
		let url = Url::parse("https://famedly.de/test/").expect("invalid url");
		let validated = validate_famedly_url(url).expect("url failed to validate");
		assert_eq!(validated.to_string(), "https://famedly.de/test/");
	}

	#[test]
	fn test_famedly_url_validate_scheme() {
		let url = Url::parse("famedly.de:443").expect("invalid url");
		assert!(validate_famedly_url(url).is_err());
	}
}

/// Configuration for the sync client
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
	/// LDAP-specific configuration
	pub ldap: LdapConfig,
	/// Configuration related to Famedly Zitadel
	pub famedly: FamedlyConfig,
	/// Opt-in features
	pub feature_flags: Set<FeatureFlag>,
	/// Where to cache the last known LDAP state
	pub cache_path: PathBuf,
	/// The sync tool log level
	pub log_level: Option<String>,
}

/// LDAP-specific configuration
#[derive(Debug, Clone, Deserialize)]
pub struct LdapConfig {
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
	/// TLS-related configuration
	pub tls: Option<LdapTlsConfig>,
}

impl From<LdapConfig> for ldap_poller::Config {
	fn from(cfg: LdapConfig) -> ldap_poller::Config {
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
#[derive(Debug, Clone, Deserialize)]
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
	/// This attribute shows the account status (LDAP = Enabled, accountStatus)
	pub status: AttributeMapping,
	/// Marks an account as enabled (LDAP = TRUE, active)
	pub enable_value: String,
	/// Marks an account as enabled (LDAP = FALSE, inactive)
	pub disable_value: String,
	/// Last modified
	pub last_modified: Option<AttributeMapping>,
}

/// How an attribute should be defined in config - it can either be a
/// raw string, *or* it can be a struct defining both an attribute
/// name and whether the attribute should be treated as binary.
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
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

/// Configuration related to Famedly Zitadel
#[derive(Debug, Clone, Deserialize)]
pub struct FamedlyConfig {
	/// The URL for Famedly authentication
	pub url: Url,
	/// File containing a private key for authentication to Famedly
	pub key_file: PathBuf,
	/// Organization ID provided by Famedly
	pub organization_id: String,
	/// Project ID provided by Famedly
	pub project_id: String,
	/// IDP ID provided by Famedly
	pub idp_id: String,
}

pub type Set<T> = Vec<T>;

/// Opt-in features
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub enum FeatureFlag {
	/// If SSO should be activated. It requires idpId, idpUserName, idpUserId
	/// mapping
	SsoLogin,
	/// If users should verify the mail. Users will receive a verification mail
	VerifyEmail,
	/// If users should verify the phone. Users will receive a verification sms
	VerifyPhone,
}
