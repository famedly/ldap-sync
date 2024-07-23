//! All sync client configuration structs and logic
use std::path::{Path, PathBuf};

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
	/// Enable StartTLS for secure communication
	pub start_tls: bool,
	/// Whether to disable tls verification
	pub no_tls_verify: bool,
	/// Path to the root certificates
	pub root_certificates_path: Option<PathBuf>,
	/// The base DN for searching users
	pub base_dn: String,
	/// The DN to bind for authentication
	pub bind_dn: String,
	/// The password for the bind DN
	pub bind_password: String,
	// /// TODO
	// /// Example: inetOrgPerson, organizationalPerson
	// userObjectClasses: Vec<String>,
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
}

impl From<LdapConfig> for ldap_poller::Config {
	fn from(cfg: LdapConfig) -> ldap_poller::Config {
		let attributes = cfg.attributes;
		ldap_poller::Config {
			url: cfg.url,
			connection: ConnectionConfig {
				timeout: cfg.timeout,
				operation_timeout: std::time::Duration::from_secs(cfg.timeout),
				tls: TLSConfig {
					starttls: cfg.start_tls,
					no_tls_verify: cfg.no_tls_verify,
					root_certificates_path: cfg.root_certificates_path,
				},
			},
			search_user: cfg.bind_dn,
			search_password: cfg.bind_password,
			searches: Searches {
				user_base: cfg.base_dn,
				user_filter: cfg.user_filter,
				page_size: None,
			},
			attributes: AttributeConfig {
				pid: attributes.user_id,
				updated: attributes.last_modified,
				additional: vec![
					attributes.first_name,
					attributes.last_name,
					attributes.preferred_username,
					attributes.email,
					attributes.phone,
				],
				attrs_to_track: vec![attributes.status],
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
	pub first_name: String,
	/// Attribute for the user's last name
	pub last_name: String,
	/// Attribute for the user's preferred username
	pub preferred_username: String,
	/// Attribute for the user's email address
	pub email: String,
	/// Attribute for the user's phone number
	pub phone: String,
	/// Attribute for the user's unique ID
	pub user_id: String,
	/// This attribute shows the account status (LDAP = Enabled, accountStatus)
	pub status: String,
	/// Marks an account as enabled (LDAP = TRUE, active)
	pub enable_value: String,
	/// Marks an account as enabled (LDAP = FALSE, inactive)
	pub disable_value: String,
	/// Last modified
	pub last_modified: Option<String>,
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
