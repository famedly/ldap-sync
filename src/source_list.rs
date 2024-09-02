//! Endpoint -> Famedly Zitadel sync tool.

use anyhow::{bail, Result};

use crate::{
	config::{FeatureFlags, SourceListConfig},
	user::User,
	Config, Source,
};

/// Disable List sync source
pub struct SourceList {
	/// Disable List configuration
	source_list_config: SourceListConfig,
	/// Optional set of features
	feature_flags: FeatureFlags,
}

impl Source for SourceList {
	fn new(config: &Config) -> Result<Self> {
		Ok(Self {
			source_list_config: match config.source_list.clone() {
				Some(source_list) => source_list,
				None => bail!("Endpoint configuration is missing"),
			},
			feature_flags: config.feature_flags.clone(),
		})
	}

	async fn get_all_changes(&self) -> Result<(Vec<User>, Vec<(User, User)>, Vec<String>)> {
		anyhow::bail!("Not implemented!");
	}

	/// Get list of user emails that have been removed
	async fn get_removed_user_emails(&self) -> Result<Vec<String>> {
		if self.feature_flags.dry_run() {
			tracing::warn!("Not fetching source list during a dry run");
			return Ok(vec![]);
		}

		// TODO
		Ok(vec![])
	}
}
