#![allow(clippy::expect_used)]

use std::{collections::HashSet, path::Path, time::Duration};

use ldap3::{Ldap as LdapClient, LdapConnAsync, LdapConnSettings, Mod};
use ldap_sync::{sync_ldap_users_to_zitadel, Config};
use tempfile::TempDir;
use test_log::test;
use tokio::sync::OnceCell;
use url::Url;
use uuid::{uuid, Uuid};
use zitadel_rust_client::{
	error::{Error as ZitadelError, TonicErrorCode},
	Type, Zitadel,
};

static CONFIG: OnceCell<Config> = OnceCell::const_new();
static TEMPDIR: OnceCell<TempDir> = OnceCell::const_new();

#[test(tokio::test)]
#[test_log(default_log_filter = "debug")]
async fn test_e2e_simple_sync() {
	let mut ldap = Ldap::new().await;
	ldap.create_user(
		"Bob",
		"Tables",
		"Bobby",
		"simple@famedly.de",
		Some("+12015550123"),
		"simple",
		false,
	)
	.await;

	sync_ldap_users_to_zitadel(config().await.clone()).await.expect("syncing failed");

	let zitadel = open_zitadel_connection().await;
	let user = zitadel
		.get_user_by_login_name("simple@famedly.de")
		.await
		.expect("could not query Zitadel users");

	assert!(user.is_some());

	let user = user.expect("could not find user");

	assert_eq!(user.user_name, "simple@famedly.de");

	if let Some(Type::Human(user)) = user.r#type {
		let profile = user.profile.expect("user lacks a profile");
		let phone = user.phone.expect("user lacks a phone number)");
		let email = user.email.expect("user lacks an email address");

		assert_eq!(profile.first_name, "Bob");
		assert_eq!(profile.last_name, "Tables");
		assert_eq!(profile.display_name, "Tables, Bob");
		assert_eq!(phone.phone, "+12015550123");
		assert!(phone.is_phone_verified);
		assert_eq!(email.email, "simple@famedly.de");
		assert!(email.is_email_verified);
	} else {
		panic!("user lacks details");
	}

	let preferred_username = zitadel
		.get_user_metadata(
			Some(config().await.famedly.organization_id.clone()),
			&user.id,
			"preferred_username",
		)
		.await
		.expect("could not get user metadata");
	assert_eq!(preferred_username, Some("Bobby".to_owned()));

	let uuid = Uuid::new_v5(&uuid!("d9979cff-abee-4666-bc88-1ec45a843fb8"), "simple".as_bytes());

	let localpart = zitadel
		.get_user_metadata(
			Some(config().await.famedly.organization_id.clone()),
			&user.id,
			"localpart",
		)
		.await
		.expect("could not get user metadata");
	assert_eq!(localpart, Some(uuid.to_string()));

	let grants = zitadel
		.list_user_grants(&config().await.famedly.organization_id, &user.id)
		.await
		.expect("failed to get user grants");

	let grant = grants.result.first().expect("no user grants found");
	assert!(grant.role_keys.clone().into_iter().any(|key| key == "User"));
}

#[test(tokio::test)]
#[test_log(default_log_filter = "debug")]
async fn test_e2e_sync_disabled_user() {
	let mut ldap = Ldap::new().await;
	ldap.create_user(
		"Bob",
		"Tables",
		"Bobby",
		"disabled_user@famedly.de",
		Some("+12015550124"),
		"disabled_user",
		true,
	)
	.await;

	sync_ldap_users_to_zitadel(config().await.clone()).await.expect("syncing failed");

	let zitadel = open_zitadel_connection().await;
	let user = zitadel.get_user_by_login_name("disabled_user@famedly.de").await;

	if let Err(error) = user {
		match error {
			ZitadelError::TonicResponseError(status)
				if status.code() == TonicErrorCode::NotFound =>
			{
				return;
			}
			_ => {
				panic!("zitadel failed while searching for user: {}", error)
			}
		}
	} else {
		panic!("disabled user was synced: {:?}", user);
	}
}

#[test(tokio::test)]
#[test_log(default_log_filter = "debug")]
async fn test_e2e_sync_change() {
	let mut ldap = Ldap::new().await;
	ldap.create_user(
		"Bob",
		"Tables",
		"Bobby2",
		"change@famedly.de",
		Some("+12015550124"),
		"change",
		false,
	)
	.await;

	sync_ldap_users_to_zitadel(config().await.clone()).await.expect("syncing failed");

	ldap.change_user("change", vec![("telephoneNumber", HashSet::from(["+12015550123"]))]).await;

	sync_ldap_users_to_zitadel(config().await.clone()).await.expect("syncing failed");

	let zitadel = open_zitadel_connection().await;
	let user = zitadel
		.get_user_by_login_name("change@famedly.de")
		.await
		.expect("could not query Zitadel users")
		.expect("missing Zitadel user");

	match user.r#type {
		Some(Type::Human(user)) => {
			assert_eq!(user.phone.expect("phone missing").phone, "+12015550123");
		}

		_ => panic!("human user became a machine user?"),
	}
}

#[test(tokio::test)]
#[test_log(default_log_filter = "debug")]
async fn test_e2e_sync_disable() {
	let mut ldap = Ldap::new().await;
	ldap.create_user(
		"Bob",
		"Tables",
		"Bobby2",
		"disable@famedly.de",
		Some("+12015550124"),
		"disable",
		false,
	)
	.await;

	sync_ldap_users_to_zitadel(config().await.clone()).await.expect("syncing failed");

	ldap.change_user("disable", vec![("shadowInactive", HashSet::from(["514"]))]).await;

	sync_ldap_users_to_zitadel(config().await.clone()).await.expect("syncing failed");

	let zitadel = open_zitadel_connection().await;
	let user = zitadel.get_user_by_login_name("disable@famedly.de").await;

	assert!(user.is_err_and(|error| matches!(error, ZitadelError::TonicResponseError(status) if status.code() == TonicErrorCode::NotFound)));
}

#[test(tokio::test)]
#[test_log(default_log_filter = "debug")]
async fn test_e2e_sync_email_change() {
	let mut ldap = Ldap::new().await;
	ldap.create_user(
		"Bob",
		"Tables",
		"Bobby2",
		"email_change@famedly.de",
		Some("+12015550124"),
		"email_change",
		false,
	)
	.await;

	sync_ldap_users_to_zitadel(config().await.clone()).await.expect("syncing failed");

	ldap.change_user("email_change", vec![("mail", HashSet::from(["email_changed@famedly.de"]))])
		.await;

	sync_ldap_users_to_zitadel(config().await.clone()).await.expect("syncing failed");

	let zitadel = open_zitadel_connection().await;
	let user = zitadel.get_user_by_login_name("email_changed@famedly.de").await;

	assert!(user.is_ok());
}

#[test(tokio::test)]
#[test_log(default_log_filter = "debug")]
async fn test_e2e_sync_deletion() {
	let mut ldap = Ldap::new().await;
	ldap.create_user(
		"bob",
		"Tables",
		"Bobby3",
		"deleted@famedly.de",
		Some("+12015550124"),
		"deleted",
		false,
	)
	.await;

	sync_ldap_users_to_zitadel(config().await.clone()).await.expect("syncing failed");

	let zitadel = open_zitadel_connection().await;
	let user =
		zitadel.get_user_by_login_name("deleted@famedly.de").await.expect("failed to find user");
	assert!(user.is_some());

	ldap.delete_user("deleted").await;

	sync_ldap_users_to_zitadel(config().await.clone()).await.expect("syncing failed");

	let user = zitadel.get_user_by_login_name("deleted@famedly.de").await;
	assert!(user.is_err_and(|error| matches!(error, ZitadelError::TonicResponseError(status) if status.code() == TonicErrorCode::NotFound)));
}

#[test(tokio::test)]
#[test_log(default_log_filter = "debug")]
async fn test_e2e_ldaps() {
	let mut config = config().await.clone();
	config.ldap.url = Url::parse("ldaps://localhost:1636").expect("invalid ldaps url");

	let mut ldap = Ldap::new().await;
	ldap.create_user(
		"Bob",
		"Tables",
		"Bobby",
		"tls@famedly.de",
		Some("+12015550123"),
		"tls",
		false,
	)
	.await;

	sync_ldap_users_to_zitadel(config).await.expect("syncing failed");

	let zitadel = open_zitadel_connection().await;
	let user = zitadel
		.get_user_by_login_name("tls@famedly.de")
		.await
		.expect("could not query Zitadel users");

	assert!(user.is_some());
}

#[test(tokio::test)]
#[test_log(default_log_filter = "debug")]
async fn test_e2e_ldaps_starttls() {
	let mut config = config().await.clone();
	config.ldap.tls.danger_use_start_tls = true;

	let mut ldap = Ldap::new().await;
	ldap.create_user(
		"Bob",
		"Tables",
		"Bobby",
		"starttls@famedly.de",
		Some("+12015550123"),
		"starttls",
		false,
	)
	.await;

	sync_ldap_users_to_zitadel(config).await.expect("syncing failed");

	let zitadel = open_zitadel_connection().await;
	let user = zitadel
		.get_user_by_login_name("starttls@famedly.de")
		.await
		.expect("could not query Zitadel users");

	assert!(user.is_some());
}

#[test(tokio::test)]
#[test_log(default_log_filter = "debug")]
async fn test_e2e_no_phone() {
	let mut ldap = Ldap::new().await;
	ldap.create_user("Bob", "Tables", "Bobby", "no_phone@famedly.de", None, "no_phone", false)
		.await;

	sync_ldap_users_to_zitadel(config().await.clone()).await.expect("syncing failed");

	let zitadel = open_zitadel_connection().await;
	let user = zitadel
		.get_user_by_login_name("no_phone@famedly.de")
		.await
		.expect("could not query Zitadel users");

	let user = user.expect("could not find user");

	if let Some(Type::Human(user)) = user.r#type {
		// Yes, I know, the codegen for the zitadel crate is
		// pretty crazy. A missing phone number is represented as
		// Some(Phone { phone: "", is_phone_Verified: _ })
		assert_eq!(user.phone.expect("user lacks a phone number object").phone, "");
	} else {
		panic!("user lacks details");
	};
}

struct Ldap {
	client: LdapClient,
}

impl Ldap {
	async fn new() -> Self {
		let config = config().await;
		let mut settings = LdapConnSettings::new();

		settings = settings.set_conn_timeout(Duration::from_secs(config.ldap.timeout));
		settings = settings.set_starttls(false);

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
		telephone_number: Option<&str>,
		uid: &str,
		shadow_inactive: bool,
	) {
		tracing::info!("Adding test user to LDAP: `{mail}``");

		let mut attrs = vec![
			("objectClass", HashSet::from(["inetOrgPerson", "shadowAccount"])),
			("cn", HashSet::from([cn])),
			("sn", HashSet::from([sn])),
			("displayName", HashSet::from([display_name])),
			("mail", HashSet::from([mail])),
			("uid", HashSet::from([uid])),
			("shadowInactive", HashSet::from([if shadow_inactive { "514" } else { "512" }])),
		];

		if let Some(phone) = telephone_number {
			attrs.push(("telephoneNumber", HashSet::from([phone])));
		};

		self.client
			.add(&format!("uid={},{}", uid, config().await.ldap.base_dn.as_str()), attrs)
			.await
			.expect("failed to create debug user")
			.success()
			.expect("failed to create debug user");

		tracing::info!("Successfully added test user");
	}

	async fn change_user(&mut self, uid: &str, changes: Vec<(&str, HashSet<&str>)>) {
		let mods = changes
			.into_iter()
			.map(|(attribute, changes)| Mod::Replace(attribute, changes))
			.collect();

		self.client
			.modify(&format!("uid={},{}", uid, config().await.ldap.base_dn.as_str()), mods)
			.await
			.expect("failed to modify user");
	}

	async fn delete_user(&mut self, uid: &str) {
		self.client
			.delete(&format!("uid={},{}", uid, config().await.ldap.base_dn.as_str()))
			.await
			.expect("failed to delete user");
	}
}

/// Open a connection to the configured Zitadel backend
async fn open_zitadel_connection() -> Zitadel {
	let famedly = config().await.famedly.clone();
	Zitadel::new(famedly.url, famedly.key_file).await.expect("failed to set up Zitadel client")
}

/// Get the module's test environment config
async fn config() -> &'static Config {
	CONFIG
		.get_or_init(|| async {
			let mut config = Config::from_file(Path::new("tests/environment/config.yaml"))
				.await
				.expect("failed to parse test env file");

			let tempdir = TEMPDIR
				.get_or_init(|| async { TempDir::new().expect("failed to initialize cache dir") })
				.await;

			config.cache_path = tempdir.path().join("cache.bin");

			config
		})
		.await
}
