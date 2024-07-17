//! Simple LDAP -> Famedly Zitadel sync tool to match users between
//! clients and our infrastructure.
use anyhow::{Context, Result};

use ldap_poller::ldap3::SearchEntry;
use ldap_poller::{ldap::EntryStatus, Ldap};
use tokio::sync::mpsc::Receiver;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

mod config;
mod zitadel;

pub use config::Config;
use zitadel::Zitadel;

/// Run the sync
pub async fn do_the_thing(config: Config) -> Result<()> {
	let zitadel = Zitadel::new(&config).await?;

	let (mut ldap_client, ldap_receiver) = Ldap::new(config.clone().ldap.into(), None);
	tokio::spawn(async move {
		ldap_client.sync_once(None).await.context("Failed to sync/fetch data from LDAP")
	});

	let (added, _changed, _removed) = get_user_changes(ldap_receiver).await;
	tracing::info!("Finished syncing LDAP data");

	zitadel.import_new_users(added).await?;

	Ok(())
}

/// Get user changes from an ldap receiver
async fn get_user_changes(
	ldap_receiver: Receiver<EntryStatus>,
) -> (Vec<SearchEntry>, Vec<SearchEntry>, Vec<Vec<u8>>) {
	ReceiverStream::new(ldap_receiver)
		.fold((vec![], vec![], vec![]), |(mut added, mut changed, mut removed), entry_status| {
			match entry_status {
				EntryStatus::New(entry) => added.push(entry),
				EntryStatus::Changed { old: _, new } => changed.push(new),
				EntryStatus::Removed(entry) => removed.push(entry),
			};
			(added, changed, removed)
		})
		.await
}
