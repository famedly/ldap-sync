//! Simple LDAP -> Famedly Zitadel sync tool to match users between
//! clients and our infrastructure.
use anyhow::{Context, Result};

use ldap_poller::{ldap::EntryStatus, Ldap};

mod config;
pub use config::Config;

/// Do the thing
pub async fn do_the_thing(config: Config) -> Result<()> {
	let (mut ldap_client, mut ldap_receiver) = Ldap::new(config.ldap.into(), None);
	tokio::spawn(async move {
		ldap_client.sync_once(None).await.context("Failed to sync/fetch data from LDAP")
	});

	while let Some(entry_status) = ldap_receiver.recv().await {
		tracing::debug!("new ldap event {:?}", &entry_status);
		match entry_status {
			EntryStatus::New(_entry) => {} // create new
			EntryStatus::Changed { new: _new_entry, old: _old_entry } => {} // update status
			EntryStatus::Removed(_id) => {} // delete
		}
	}

	tracing::info!("Finished syncing LDAP data");

	Ok(())
}
