#![allow(clippy::expect_used)]

use std::{collections::HashSet, path::Path, time::Duration};

use ldap3::{Ldap as LdapClient, LdapConnAsync, LdapConnSettings};
use ldap_sync::{do_the_thing, Config};
use test_log::test;
use tokio::sync::OnceCell;
use zitadel_rust_client::{Config as ZitadelConfig, Zitadel};

static CONFIG: OnceCell<Config> = OnceCell::const_new();

#[test(tokio::test)]
#[test_log(default_log_filter = "debug")]
async fn test_e2e_simple_sync() {
	let mut ldap = Ldap::new().await;
	ldap.create_user(
		"Bob",
		"Tables",
		"Bobby",
		"bob.tables@famedly.de",
		"+12015550123",
		"bobby",
		false,
	)
	.await;

	do_the_thing(config().await.clone()).await.expect("syncing failed");

	let zitadel = open_zitadel_connection().await;
	let user = zitadel
		.get_user_by_login_name("bobby@famedly.de")
		.await
		.expect("could not query Zitadel users");

	assert!(user.is_some());
}

struct Ldap {
	client: LdapClient,
}

impl Ldap {
	async fn new() -> Self {
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

		Self { client: ldap }
	}

	/// Create a test user
	#[allow(clippy::too_many_arguments)]
	async fn create_user(
		&mut self,
		cn: &str,
		sn: &str,
		display_name: &str,
		mail: &str,
		telephone_number: &str,
		uid: &str,
		shadow_inactive: bool,
	) {
		tracing::info!("Adding test user to LDAP: `{mail}``");

		self.client
			.add(
				&format!("cn={},{}", cn, config().await.ldap.base_dn.as_str()),
				vec![
					("objectClass", HashSet::from(["inetOrgPerson", "shadowAccount"])),
					("cn", HashSet::from([cn])),
					("sn", HashSet::from([sn])),
					("displayName", HashSet::from([display_name])),
					("mail", HashSet::from([mail])),
					("telephoneNumber", HashSet::from([telephone_number])),
					("uid", HashSet::from([uid])),
					(
						"shadowInactive",
						HashSet::from([if shadow_inactive { "512" } else { "514" }]),
					),
				],
			)
			.await
			.expect("failed to create debug user")
			.success()
			.expect("failed to create debug user");

		tracing::info!("Successfully added test user");
	}
}

/// Open a connection to the configured Zitadel backend
async fn open_zitadel_connection() -> Zitadel {
	let famedly = config().await.famedly.clone();
	let zitadel_config = ZitadelConfig::new(famedly.url, famedly.key_file);

	Zitadel::new(&zitadel_config).await.expect("failed to set up Zitadel client")
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
