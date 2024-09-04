//! Sync tool between other sources and our infrastructure based on Zitadel.

use anyhow::Result;

mod config;
mod sources;
mod user;
mod zitadel;

pub use config::{Config, FeatureFlag};
pub use sources::ldap::AttributeMapping;
use sources::{ldap::SourceLdap, ukt::SourceUkt};
use zitadel::Zitadel;

/// Perform a sync operation
pub async fn perform_sync(config: &Config) -> Result<()> {
	if !config.feature_flags.is_enabled(FeatureFlag::SsoLogin) {
		anyhow::bail!("Non-SSO configuration is currently not supported");
	}

	// Setup Zitadel client
	let zitadel = Zitadel::new(config).await?;

	// Perform LDAP sync
	if config.source_ldap.is_some() {
		let ldap_sync = SourceLdap::new(config)?;
		let ldap_changes = ldap_sync.get_all_changes().await?;

		if !config.feature_flags.is_enabled(FeatureFlag::DeactivateOnly) {
			zitadel.import_new_users(ldap_changes.new_users).await?;
			zitadel.delete_users_by_id(ldap_changes.deleted_user_ids).await?;
		}

		zitadel.update_users(ldap_changes.changed_users).await?;
	}

	// Perform UKT sync
	if config.source_ukt.is_some() {
		let endpoint_sync = SourceUkt::new(config)?;
		let removed = endpoint_sync.get_removed_user_emails().await?;
		zitadel.delete_users_by_email(removed).await?;
	}

	Ok(())
}
