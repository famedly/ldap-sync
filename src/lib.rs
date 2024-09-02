//! Sync tool between other sources and our infrastructure based on Zitadel.

use anyhow::Result;

mod config;
mod source_ldap;
mod source_list;
mod user;
mod zitadel;

pub use config::{AttributeMapping, Config, FeatureFlag};
use source_ldap::SourceLdap;
use source_list::SourceList;
use user::User;
use zitadel::Zitadel;

/// Trait to define sources that can be used for syncing
trait Source {
	/// Create a new sync source
	fn new(config: &Config) -> Result<Self>
	where
		Self: Sized;

	/// Get lists of all changes
	async fn get_all_changes(&self) -> Result<(Vec<User>, Vec<(User, User)>, Vec<String>)>;

	/// Get list of user emails that have been removed
	async fn get_removed_user_emails(&self) -> Result<Vec<String>>;
}

/// Perform a sync operation
pub async fn perform_sync(config: &Config) -> Result<()> {
	if !config.feature_flags.require_sso_login() {
		anyhow::bail!("Non-SSO configuration is currently not supported");
	}

	// Setup Zitadel client
	let zitadel = Zitadel::new(config).await?;

	// Perform LDAP sync
	if config.source_ldap.is_some() {
		let ldap_sync = SourceLdap::new(config)?;
		let (added, changed, removed) = ldap_sync.get_all_changes().await?;

		if !config.feature_flags.deactivate_only() {
			zitadel.import_new_users(added).await?;
			zitadel.delete_users_by_id(removed).await?;
		}

		zitadel.update_users(changed).await?;
	}

	// Perform Disable List sync
	if config.source_list.is_some() {
		let endpoint_sync = SourceList::new(config)?;
		let removed = endpoint_sync.get_removed_user_emails().await?;
		zitadel.delete_users_by_email(removed).await?;
	}

	Ok(())
}
