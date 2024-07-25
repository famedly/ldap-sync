//! Simple LDAP -> Famedly Zitadel sync tool to match users between
//! clients and our infrastructure.
use std::path::Path;

use anyhow::{bail, Context, Result};
use ldap_poller::{ldap::EntryStatus, ldap3::SearchEntry, Cache, Ldap};
use tokio::sync::mpsc::Receiver;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

mod config;
mod zitadel;

pub use config::Config;
use zitadel::Zitadel;

/// Run the sync
pub async fn do_the_thing(config: Config) -> Result<()> {
	let cache = read_cache(&config.cache_path).await?;
	let zitadel = Zitadel::new(&config).await?;
	let (mut ldap_client, ldap_receiver) = Ldap::new(config.clone().ldap.into(), cache);

	let sync_handle: tokio::task::JoinHandle<Result<_>> = tokio::spawn(async move {
		ldap_client.sync_once(None).await.context("failed to sync/fetch data from LDAP")?;
		let cache = ldap_client.persist_cache().await;
		tokio::fs::write(
			&config.cache_path,
			bincode::serialize(&cache).context("failed to serialize cache")?,
		)
		.await
		.context("failed to write cache")?;

		Ok(())
	});

	let (added, changed, _removed) = get_user_changes(ldap_receiver).await;
	tracing::info!("Finished syncing LDAP data");

	zitadel.import_new_users(added).await?;
	zitadel.update_users(changed).await?;

	let _ = sync_handle.await?;

	Ok(())
}

/// Get user changes from an ldap receiver
async fn get_user_changes(
	ldap_receiver: Receiver<EntryStatus>,
) -> (Vec<SearchEntry>, Vec<(SearchEntry, SearchEntry)>, Vec<Vec<u8>>) {
	ReceiverStream::new(ldap_receiver)
		.fold((vec![], vec![], vec![]), |(mut added, mut changed, mut removed), entry_status| {
			match entry_status {
				EntryStatus::New(entry) => {
					tracing::debug!("New entry: {:?}", entry);
					added.push(entry);
				}
				EntryStatus::Changed { old, new } => changed.push((old, new)),
				EntryStatus::Removed(entry) => removed.push(entry),
			};
			(added, changed, removed)
		})
		.await
}

/// Read the ldap sync cache
async fn read_cache(path: &Path) -> Result<Option<Cache>> {
	Ok(match tokio::fs::read(path).await {
		Ok(data) => Some(bincode::deserialize(&data).context("cache deserialization failed")?),
		Err(err) => {
			if err.kind() == std::io::ErrorKind::NotFound {
				tracing::info!("LDAP sync cache missing");
				None
			} else {
				bail!(err)
			}
		}
	})
}
