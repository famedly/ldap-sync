#![allow(clippy::expect_used)]

use std::{collections::HashSet, path::Path, time::Duration};

use ldap3::{Ldap, LdapConnAsync, LdapConnSettings};
use ldap_sync::{do_the_thing, Config};
use test_log::test;
use tokio::sync::OnceCell;

static CONFIG: OnceCell<Config> = OnceCell::const_new();

#[test(tokio::test)]
#[test_log(default_log_filter = "debug")]
async fn test_e2e_simple_sync() {
	let mut ldap = open_ldap_connection().await;

	ldap.add(
		&format!("cn=Bob,{}", config().await.ldap.base_dn.as_str()),
		vec![
			("objectClass", HashSet::from(["inetOrgPerson", "shadowAccount"])),
			("cn", HashSet::from(["Bob"])),
			("sn", HashSet::from(["Tables"])),
			("displayName", HashSet::from(["Bobby"])),
			("mail", HashSet::from(["bobby@famedly.de"])),
			("telephoneNumber", HashSet::from(["+1-201-555-0123"])),
			("uid", HashSet::from(["bobby"])),
			("shadowInactive", HashSet::from(["512"])),
		],
	)
	.await
	.expect("failed to create debug user")
	.success()
	.expect("failed to create debug user");

	tracing::info!("Successfully added test user");

	ldap.unbind().await.expect("failed to disconnect from ldap");

	do_the_thing(config().await.clone()).await.expect("syncing failed");
}

/// Open an ldap connection to the configured ldap backend
async fn open_ldap_connection() -> Ldap {
	let config = config().await;
	let mut settings = LdapConnSettings::new();

	settings = settings.set_conn_timeout(Duration::from_secs(config.ldap.timeout));
	settings = settings.set_starttls(config.ldap.start_tls);
	// We assume that the test instances aren't spoofing certificates
	// or anything - asserting tls verification works is up to the
	// tests, not the setup helper.
	settings = settings.set_no_tls_verify(true);

	let (conn, mut ldap) = LdapConnAsync::from_url_with_settings(settings, &config.ldap.url)
		.await
		.expect("could not connect to ldap");

	ldap3::drive!(conn);

	ldap.simple_bind(&config.ldap.bind_dn, &config.ldap.bind_password)
		.await
		.expect("could not authenticate to ldap");

	ldap
}

/// Get the module's test environment config
async fn config() -> &'static Config {
	CONFIG
		.get_or_init(|| async {
			Config::from_file(Path::new("tests/environment/config.yaml"))
				.await
				.expect("failed to parse test env file")
		})
		.await
}
