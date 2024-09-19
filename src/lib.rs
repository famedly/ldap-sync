//! Sync tool between other sources and our infrastructure based on Zitadel.

mod config;
mod sources;
mod user;
mod zitadel;

pub use config::{Config, FeatureFlag};
pub use sources::{
	csv::test_helpers as csv_test_helpers, ldap::AttributeMapping,
	ukt::test_helpers as ukt_test_helpers,
};
