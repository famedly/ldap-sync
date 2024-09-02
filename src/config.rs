//! All sync client configuration structs and logic
use std::{
	fmt::Display,
	ops::{Deref, DerefMut},
	path::{Path, PathBuf},
};

use anyhow::{bail, Result};
use ldap_poller::{config::TLSConfig, AttributeConfig, CacheMethod, ConnectionConfig, Searches};
use serde::Deserialize;
use url::Url;

/// App prefix for env var configuration
const ENV_VAR_CONFIG_PREFIX: &str = "FAMEDLY_LDAP_SYNC";
/// Separator for setting a list using env vars
const ENV_VAR_LIST_SEP: &str = " ";

/// Configuration for the sync client
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Config {
	/// Configuration related to Zitadel provided by Famedly
	// TODO: Renamed from famedly to zitadel_config (needs update to the env vars)
	pub zitadel_config: ZitadelConfig,
	/// Optional LDAP configuration
	// TODO: Renamed from ldap to source_ldap (needs update to the env vars)
	pub source_ldap: Option<SourceLdapConfig>,
	/// Optional Disable List configuration
	pub source_list: Option<SourceListConfig>,
	/// Optional sync tool log level
	pub log_level: Option<String>,
	/// Opt-in features
	#[serde(default)]
	pub feature_flags: FeatureFlags,
	/// General cache path
	pub cache_path: PathBuf,
}

impl Config {
	/// Create new config from file and env var
	pub fn new(path: &Path) -> Result<Self> {
		let config_builder = config::Config::builder()
			.add_source(config::File::from(path).required(false))
			.add_source(
				config::Environment::with_prefix(ENV_VAR_CONFIG_PREFIX)
					.separator("__")
					.list_separator(ENV_VAR_LIST_SEP)
					.with_list_parse_key("source_ldap.attributes.disable_bitmasks")
					.with_list_parse_key("feature_flags")
					.try_parsing(true),
			);

		let config_builder = config_builder.build()?;

		let config: Config = config_builder.try_deserialize()?;

		config.validate()
	}

	/// Validate the config and return a valid configuration
	fn validate(mut self) -> Result<Self> {
		self.zitadel_config.url = validate_zitadel_url(self.zitadel_config.url)?;

		Ok(self)
	}
}

/// Configuration related to Famedly Zitadel
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ZitadelConfig {
	/// The URL for Famedly Zitadel authentication
	pub url: Url,
	/// File containing a private key for authentication to Famedly Zitadel
	pub key_file: PathBuf,
	/// Organization ID provided by Famedly Zitadel
	pub organization_id: String,
	/// Project ID provided by Famedly Zitadel
	pub project_id: String,
	/// IDP ID provided by Famedly Zitadel
	pub idp_id: String,
}

/// Configuration to get a list of users from an endpoint
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SourceListConfig {
	/// The URL of the endpoint
	pub url: Url,
	/// The API client ID for the endpoint
	pub client_id: String,
	/// The API client secret for the endpoint
	pub client_secret: String,
	/// The scope for the endpoint
	pub scope: String,
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

/// Opt-in features for LDAP
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FeatureFlag {
	/// If SSO should be activated. It requires idpId, idpUserName, idpUserId
	/// mapping
	SsoLogin,
	/// If users should verify the mail. Users will receive a verification mail
	VerifyEmail,
	/// If users should verify the phone. Users will receive a verification sms
	VerifyPhone,
	/// If set, only log changes instead of writing anything
	DryRun,
	/// If only deactivated users should be synced
	DeactivateOnly,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct FeatureFlags(Vec<FeatureFlag>);

impl Deref for FeatureFlags {
	type Target = Vec<FeatureFlag>;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl DerefMut for FeatureFlags {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

impl FeatureFlags {
	/// Whether SSO login is enabled
	#[must_use]
	pub fn require_sso_login(&self) -> bool {
		self.contains(&FeatureFlag::SsoLogin)
	}

	/// Whether phone verification is enabled
	#[must_use]
	pub fn require_phone_verification(&self) -> bool {
		self.contains(&FeatureFlag::VerifyPhone)
	}

	/// Whether email verification is enabled
	#[must_use]
	pub fn require_email_verification(&self) -> bool {
		self.contains(&FeatureFlag::VerifyEmail)
	}

	/// Whether dry run is enabled
	#[must_use]
	pub fn dry_run(&self) -> bool {
		self.contains(&FeatureFlag::DryRun)
	}

	/// Whether LDAP deactivate only is enabled
	#[must_use]
	pub fn deactivate_only(&self) -> bool {
		self.contains(&FeatureFlag::DeactivateOnly)
	}
}

/// Validate the Zitadel URL provided by Famedly
fn validate_zitadel_url(url: Url) -> Result<Url> {
	// If a URL contains a port, the domain name may appear as a
	// scheme and pass through URL parsing despite lacking a scheme
	if url.scheme() != "https" && url.scheme() != "http" {
		bail!("zitadel URL scheme must be `http` or `https`, e.g. `https://{}`", url);
	}

	Ok(url)
}

// Run these tests with
// RUST_TEST_THREADS=1 cargo test --lib
#[cfg(test)]
mod tests {
	#![allow(clippy::expect_used, clippy::unwrap_used)]
	use std::{collections::BTreeMap, env, fs::File, io::Write};

	use indoc::indoc;
	use tempfile::TempDir;

	use super::*;

	const EXAMPLE_CONFIG_WITH_LDAP: &str = indoc! {r#"
        source_list:
          url: https://list.example.invalid
          client_id: 123456
          client_secret: abcdef
          scope: "read-maillist"

        source_ldap:
          url: ldap://localhost:1389
          base_dn: ou=testorg,dc=example,dc=org
          bind_dn: cn=admin,dc=example,dc=org
          bind_password: adminpassword
          user_filter: "(objectClass=shadowAccount)"
          timeout: 5
          check_for_deleted_entries: true
          use_attribute_filter: true
          attributes:
            first_name: "cn"
            last_name: "sn"
            preferred_username: "displayName"
            email: "mail"
            phone: "telephoneNumber"
            user_id: "uid"
            status:
              name: "shadowFlag"
              is_binary: false
            disable_bitmasks: [0x2, 0x10]
          tls:
            client_key: ./tests/environment/certs/client.key
            client_certificate: ./tests/environment/certs/client.crt
            server_certificate: ./tests/environment/certs/server.crt
            danger_disable_tls_verify: false
            danger_use_start_tls: false

        zitadel_config:
          url: http://localhost:8080
          key_file: tests/environment/zitadel/service-user.json
          organization_id: 1
          project_id: 1
          idp_id: 1

        feature_flags: []
        cache_path: ./test
	"#};

	fn full_config_example() -> Config {
		serde_yaml::from_str(EXAMPLE_CONFIG_WITH_LDAP).expect("invalid config")
	}

	fn config_without_source_ldap() -> Config {
		let mut config: BTreeMap<String, serde_yaml::Value> =
			serde_yaml::from_str(EXAMPLE_CONFIG_WITH_LDAP).expect("invalid config");
		config.remove("source_ldap");
		let slim_config = serde_yaml::to_string(&config).expect("failed to serialize config");
		serde_yaml::from_str(&slim_config).expect("invalid config")
	}

	fn config_without_source_list() -> Config {
		let mut config: BTreeMap<String, serde_yaml::Value> =
			serde_yaml::from_str(EXAMPLE_CONFIG_WITH_LDAP).expect("invalid config");
		config.remove("source_list");
		let slim_config = serde_yaml::to_string(&config).expect("failed to serialize config");
		serde_yaml::from_str(&slim_config).expect("invalid config")
	}

	fn example_ldap_config() -> SourceLdapConfig {
		let config: Config =
			serde_yaml::from_str(EXAMPLE_CONFIG_WITH_LDAP).expect("invalid config");
		config.source_ldap.expect("Expected LDAP config")
	}

	fn example_env_vars() -> Vec<(String, String)> {
		let config: serde_yaml::Value =
			serde_yaml::from_str(EXAMPLE_CONFIG_WITH_LDAP).expect("invalid config");
		let mut prefix_stack = Vec::new();
		get_env_vars_from_map(
			config.as_mapping().expect("Expected a map but it isn't"),
			&mut prefix_stack,
		)
	}

	fn get_string(value: &serde_yaml::Value) -> String {
		match value {
			serde_yaml::Value::Bool(value) => value.to_string(),
			serde_yaml::Value::Number(value) => value.to_string(),
			serde_yaml::Value::String(value) => value.to_string(),
			serde_yaml::Value::Sequence(arr) => {
				let mut values: Vec<String> = Vec::new();
				for value in arr {
					values.push(get_string(value));
				}
				values.join(ENV_VAR_LIST_SEP)
			}
			_ => "".to_owned(),
		}
	}

	fn get_env_vars_from_map(
		map: &serde_yaml::Mapping,
		prefix_stack: &mut Vec<String>,
	) -> Vec<(String, String)> {
		let mut ret = Vec::new();
		for (key, value) in map {
			let key = key.as_str().expect("Key should be a str").to_owned().to_uppercase();
			if value.is_mapping() {
				prefix_stack.push(key);
				ret.append(&mut get_env_vars_from_map(value.as_mapping().unwrap(), prefix_stack));
				let _ = prefix_stack.pop();
			} else {
				let var_key = if prefix_stack.is_empty() {
					format!("{ENV_VAR_CONFIG_PREFIX}__{key}")
				} else {
					format!(
						"{ENV_VAR_CONFIG_PREFIX}__{}__{key}",
						prefix_stack.join("__").to_uppercase()
					)
				};
				let var_value = get_string(value);
				ret.push((var_key, var_value));
			}
		}
		ret
	}

	fn create_config_file(dir: &Path) -> PathBuf {
		let file_path = dir.join("config.yaml");
		let mut config_file = File::create(&file_path).expect("failed to create config file");
		config_file
			.write_all(EXAMPLE_CONFIG_WITH_LDAP.as_bytes())
			.expect("Failed to write config file content");
		file_path
	}

	#[test]
	fn test_zitadel_url_validate_valid() {
		let url = Url::parse("https://famedly.de").expect("invalid url");
		let validated = validate_zitadel_url(url).expect("url failed to validate");
		assert_eq!(validated.to_string(), "https://famedly.de/");
	}

	#[test]
	fn test_zitadel_url_validate_trailing_slash_path() {
		let url = Url::parse("https://famedly.de/test/").expect("invalid url");
		let validated = validate_zitadel_url(url).expect("url failed to validate");
		assert_eq!(validated.to_string(), "https://famedly.de/test/");
	}

	#[test]
	fn test_zitadel_url_validate_scheme() {
		let url = Url::parse("famedly.de:443").expect("invalid url");
		assert!(validate_zitadel_url(url).is_err());
	}

	#[test]
	fn test_attribute_filter_use() {
		let ldap_config = example_ldap_config();

		assert_eq!(
			Into::<ldap_poller::Config>::into(ldap_config).attributes.get_attr_filter(),
			vec!["uid", "shadowFlag", "cn", "sn", "displayName", "mail", "telephoneNumber"]
		);
	}

	#[test]
	fn test_no_attribute_filters() {
		let mut ldap_config = example_ldap_config();
		ldap_config.use_attribute_filter = false;

		assert_eq!(
			Into::<ldap_poller::Config>::into(ldap_config).attributes.get_attr_filter(),
			vec!["*"]
		);
	}

	#[tokio::test]
	async fn test_sample_config() {
		let config = Config::new(Path::new("./config.sample.yaml"));

		assert!(config.is_ok(), "Invalid config: {:?}", config);
	}

	#[test]
	fn test_config_from_file() {
		let tempdir = TempDir::new().expect("failed to initialize cache dir");
		let file_path = create_config_file(tempdir.path());
		let config = Config::new(file_path.as_path()).expect("Failed to create config object");

		assert_eq!(full_config_example(), config);
	}

	#[test]
	fn test_config_env_var_override() {
		let tempdir = TempDir::new().expect("failed to initialize cache dir");
		let file_path = create_config_file(tempdir.path());

		let env_var_name = format!("{ENV_VAR_CONFIG_PREFIX}__SOURCE_LDAP__TIMEOUT");
		env::set_var(&env_var_name, "1");

		let loaded_config =
			Config::new(file_path.as_path()).expect("Failed to create config object");
		env::remove_var(env_var_name);

		let mut sample_config = full_config_example();
		if let Some(ref mut ldap_config) = sample_config.source_ldap {
			ldap_config.timeout = 1;
		} else {
			panic!("LDAP configuration is missing");
		}

		assert_eq!(sample_config, loaded_config);
	}

	#[test]
	fn test_no_config_file() {
		let env_vars = example_env_vars();
		for (key, value) in &env_vars {
			if !value.is_empty() {
				env::set_var(key, value);
			}
		}

		let config =
			Config::new(Path::new("no_file.yaml")).expect("Failed to create config object");

		for (key, _) in &env_vars {
			env::remove_var(key);
		}

		assert_eq!(full_config_example(), config);
	}

	#[test]
	fn test_config_env_var_feature_flag() {
		let tempdir = TempDir::new().expect("failed to initialize cache dir");
		let file_path = create_config_file(tempdir.path());

		let env_var_name = format!("{ENV_VAR_CONFIG_PREFIX}__FEATURE_FLAGS");
		env::set_var(&env_var_name, "sso_login verify_email verify_phone dry_run deactivate_only");

		let loaded_config =
			Config::new(file_path.as_path()).expect("Failed to create config object");
		let mut sample_config = full_config_example();

		sample_config.feature_flags.push(FeatureFlag::SsoLogin);
		sample_config.feature_flags.push(FeatureFlag::VerifyEmail);
		sample_config.feature_flags.push(FeatureFlag::VerifyPhone);
		sample_config.feature_flags.push(FeatureFlag::DryRun);
		sample_config.feature_flags.push(FeatureFlag::DeactivateOnly);

		env::remove_var(env_var_name);

		assert_eq!(sample_config, loaded_config);
	}
}
