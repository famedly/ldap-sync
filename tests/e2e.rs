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
		"uid=bobby,ou=people,dc=example,dc=com",
		vec![
			("givenname", HashSet::from(["Bob"])),
			("sn", HashSet::from(["Wopper"])),
			("cn", HashSet::from(["Bobby"])),
			("mail", HashSet::from(["bobby@famedly.de"])),
			("entryuuid", HashSet::from(["8bd4ac58-c5e9-4e9e-b937-35f5a764874d"])),
			("telephonenumber", HashSet::from(["+4255123541"])),
			("useraccountcontrol", HashSet::from(["512"])),
		],
	)
	.await
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

	ldap.simple_bind("cn=admin,ou=people,dc=example,dc=com", "password")
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
