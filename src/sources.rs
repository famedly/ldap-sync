//! Sources of data we want to sync from.

use anyhow::Result;
use async_trait::async_trait;

use crate::zitadel::SourceDiff;

pub mod ldap;
pub mod ukt;

/// A source of data we want to sync from.
#[async_trait]
pub trait Source {
	/// Get source name for debugging.
	fn get_name(&self) -> &'static str;

	/// Get changes from the source.
	async fn get_diff(&self) -> Result<SourceDiff>;
}
