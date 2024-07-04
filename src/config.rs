use std::{path::PathBuf, time::Duration};

use ldap_poller::{
	config::TLSConfig, ldap::EntryStatus, AttributeConfig, Cache, CacheMethod, ConnectionConfig,
	Ldap, SearchEntry, SearchEntryExt, Searches,
};
use serde::Deserialize;
use tracing::level_filters::LevelFilter;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
	pub ldap: LdapConfig,
	pub famedly: FamedlyConfig,
	pub feature_flags: Set<FeatureFlag>,
	pub cache_path: String,
	pub log_level: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LdapConfig {
	/// The URL of the LDAP/AD server
	pub url: url::Url,
	/// Enable StartTLS for secure communication
	pub start_tls: bool,
	pub no_tls_verify: bool,
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
	/// TODO
	pub attributes: LdapAttributesMapping,
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
				// TODO: add all
				additional: vec![attributes.first_name, attributes.last_name],
				attrs_to_track: vec![attributes.status],
			},
			cache_method: CacheMethod::ModificationTime,
			check_for_deleted_entries: cfg.check_for_deleted_entries,
		}
	}
}

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
	pub last_modified: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FamedlyConfig {
	/// The URL for Famedly authentication
	pub url: String,
	/// Client ID provided by Famedly
	pub client_id: String,
	/// Client secret provided by Famedly
	pub client_secret: String,
	/// Organization ID provided by Famedly
	pub organization_id: String,
	/// Project ID provided by Famedly
	pub project_id: String,
	/// IDP ID provided by Famedly
	pub idp_id: String,
}

pub type Set<T> = Vec<T>;

#[derive(Debug, Clone, Deserialize)]
pub enum FeatureFlag {
	/// If SSO should be activated. It requires idpId, idpUserName, idpUserId
	/// mapping
	SsoLogin,
	/// If users should verify the mail. Users will receive a verification mail
	VerifyEmail,
	/// If users should verify the phone. Users will receive a verification sms
	VerifyPhone,
}
