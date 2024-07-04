use std::{path::PathBuf, time::Duration};
mod config;
use anyhow::{bail, Context, Result};
pub use config::Config;
use ldap_poller::{
	config::TLSConfig, ldap::EntryStatus, AttributeConfig, Cache, CacheMethod, ConnectionConfig,
	Ldap, SearchEntry, SearchEntryExt, Searches,
};

pub async fn do_the_thing(
	Config { ldap, famedly, feature_flags, cache_path, .. }: Config,
) -> Result<()> {
	let cache: Option<Cache> = match tokio::fs::read(cache_path.clone()).await {
		Ok(data) => Some(bincode::deserialize(&data).context("Cache deserialization error")?),
		Err(err) => {
			if err.kind() == std::io::ErrorKind::NotFound {
				tracing::info!("LDAP sync cache not found");
				None
			} else {
				bail!(err);
			}
		}
	};

	let (mut client, mut receiver) = Ldap::new(ldap.into(), cache);

	let task_result: tokio::task::JoinHandle<Result<()>> = tokio::spawn(async move {
		client.sync_once(None).await.context("Failed to sync/fetch data from LDAP")?;
		let cache = client.persist_cache().await;
		tokio::fs::write(
			cache_path,
			bincode::serialize(&cache).context("Failed to serialize cache")?,
		)
		.await
		.context("Failed to write cache")?;
		Ok(())
	});

	while let Some(entry_status) = receiver.recv().await {
		tracing::debug!("new ldap event {:?}", &entry_status);
		match entry_status {
			EntryStatus::New(entry) => {} // create new
			EntryStatus::Changed { new: new_entry, old: old_entry } => {} // update status
			EntryStatus::Removed(id) => {} // delete
		}
	}
	Ok(())
}

/*
struct FamedlyLdapConn {
  url: String,
  connection_timeout: Option<Duration>,
  start_tls: Option<bool>,
  no_tls_verify: Option<bool>,

  simple_bind: Option<SimpleBind>,
  sasl_external_bind: bool,
  sasl_gssapi_bind: Option<String>,
}

struct SimpleBind {
  bind_dn: String,
  bind_pw: String,
}

async fn do_the_thing(cfg: FamedlyLdapConn) {
  let mut s = LdapConnSettings::new();
  if let Some(t) = cfg.connection_timeout {s = s.set_conn_timeout(t);}
  if let Some(t) = cfg.start_tls {s = s.set_starttls(t);}
  if let Some(t) = cfg.set_no_tls_verify {s = s.set_no_tls_verify(t);}

  let (conn, mut ldap) = LdapConnAsync::with_settings(s, &cfg.url).await.unwrap();

  if let Some(simple_bind) = cfg.simple_bind {
	ldap.simple_bind(&simple_bind.bind_dn, &simple_bind.bind_pw).await.unwrap().success().unwrap();
  }

  if cfg.sasl_external_bind {
	ldap.sasl_external_bind().await.unwrap().success().unwrap();
  }

  if let Some(server_fqdn) = cfg.sasl_gssapi_bind {
	ldap.sasl_gssapi_bind(&server_fqdn).await.unwrap().success().unwrap();
  }

  // ...

  ldap.unbind().await.unwrap();
  conn.drive().await.unwrap();
}
*/
