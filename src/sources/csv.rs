//! CSV source for us to sync with Famedly Zitadel.

use std::{fs::File, path::PathBuf};

use anyhow::{bail, Context, Result};
use csv::Reader;
use serde::Deserialize;

use crate::{config::FeatureFlags, user::User, Config, FeatureFlag};

/// CSV Source
pub struct SourceCsv {
	/// CSV Source configuration
	source_csv_config: SourceCsvConfig,
	/// Optional set of features
	feature_flags: FeatureFlags,
}

impl SourceCsv {
	/// Create a new SourceCsv instance
	pub fn new(config: &Config) -> Result<Self> {
		Ok(Self {
			source_csv_config: match config.source_csv.clone() {
				Some(source_csv) => source_csv,
				None => bail!("CSV configuration is missing"),
			},
			feature_flags: config.feature_flags.clone(),
		})
	}

	/// Get list of users from CSV file
	pub fn read_csv(&self) -> Result<Vec<User>> {
		if self.feature_flags.is_enabled(FeatureFlag::DryRun) {
			tracing::warn!("Not reading CSV during a dry run");
			return Ok(vec![]);
		}

		let file_path = &self.source_csv_config.file_path;
		let file = File::open(&self.source_csv_config.file_path)
			.context(format!("Failed to open CSV file {}", file_path.to_string_lossy()))?;
		let mut reader = Reader::from_reader(file);
		let users: Result<Vec<CsvData>, _> = reader.deserialize().collect();
		let users: Result<Vec<User>, _> = users
			.context("Failed to deserialize CSV data")
			.map(|csv_data_vec| csv_data_vec.into_iter().map(Into::into).collect());
		users
	}
}

/// Configuration to get a list of users from a CSV file
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SourceCsvConfig {
	/// The path to the CSV file
	pub file_path: PathBuf,
}

/// CSV data structure
#[derive(Debug, Deserialize)]
struct CsvData {
	/// The user's first name
	first_name: String,
	/// The user's last name
	last_name: String,
	/// The user's preferred username
	username: String,
	/// The user's email address
	email: String,
	/// The user's LDAP ID
	ldap_id: String,
	/// The user's phone number
	phone: String,
	/// Whether the user is enabled
	enabled: bool,
	/// Whether the user should be prompted to verify their email
	needs_email_verification: bool,
	/// Whether the user should be prompted to verify their phone number
	needs_phone_verification: bool,
}

impl From<CsvData> for User {
	fn from(data: CsvData) -> Self {
		User {
			first_name: data.first_name.into(),
			last_name: data.last_name.into(),
			preferred_username: data.username.into(),
			email: data.email.into(),
			ldap_id: data.ldap_id.into(),
			phone: Some(data.phone.into()),
			enabled: data.enabled,
			needs_email_verification: data.needs_email_verification,
			needs_phone_verification: data.needs_phone_verification,
			idp_id: None,
		}
	}
}

#[cfg(test)]
mod tests {
	#![allow(clippy::expect_used, clippy::unwrap_used)]
	use std::fs::write;

	use indoc::indoc;
	use tempfile::NamedTempFile;

	use super::*;
	use crate::user::StringOrBytes;

	const EXAMPLE_CONFIG: &str = indoc! {r#"
        source_csv:
          file_path: ./test_users.csv

        zitadel_config:
          url: http://localhost:8080
          key_file: tests/environment/zitadel/service-user.json
          organization_id: 1
          project_id: 1
          idp_id: 1

        feature_flags: []
        cache_path: ./test
    "#};

	fn load_config() -> Config {
		serde_yaml::from_str(EXAMPLE_CONFIG).expect("invalid config")
	}

	fn prepare_temp_csv_file(config: &mut Config, csv_content: &str) -> NamedTempFile {
		let temp_file = NamedTempFile::new().expect("Failed to create temp file");
		write(temp_file.path(), csv_content).expect("Failed to write CSV content");

		if let Some(source_csv) = config.source_csv.as_mut() {
			source_csv.file_path = temp_file.path().to_path_buf();
		}

		temp_file
	}

	#[test]
	fn test_get_users() {
		let mut config = load_config();
		let csv_content = indoc! {r#"
          first_name,last_name,username,email,ldap_id,phone,enabled,needs_email_verification,needs_phone_verification
          John,Doe,jdoe,john.doe@example.com,jdoe,111-111-1111,true,false,false
          Jane,Smith,jasmith,jane.smith@example.com,jasmith,222-222-2222,true,false,false
          Alice,Johnson,ajohnson,alice.johnson@example.com,ajohnson,333-333-3333,true,false,false
          Bob,Williams,bwilliams,bob.williams@example.com,bwilliams,444-444-4444,true,false,false
        "#};
		let _file = prepare_temp_csv_file(&mut config, csv_content);

		let source_csv = SourceCsv::new(&config).expect("Failed to create SourceCsv");

		let result = source_csv.read_csv();
		assert!(result.is_ok(), "Failed to get users: {:?}", result);

		let users = result.expect("Failed to get users");
		assert_eq!(users.len(), 4, "Unexpected number of users");
		assert_eq!(
			users[0].first_name,
			StringOrBytes::String("John".to_owned()),
			"Unexpected first name at index 0"
		);
		assert_eq!(
			users[0].email,
			StringOrBytes::String("john.doe@example.com".to_owned()),
			"Unexpected email at index 0"
		);
		assert_eq!(
			users[3].last_name,
			StringOrBytes::String("Williams".to_owned()),
			"Unexpected last name at index 3"
		);
		assert_eq!(
			users[3].ldap_id,
			StringOrBytes::String("bwilliams".to_owned()),
			"Unexpected ldap_id at index 3"
		);
	}

	#[test]
	fn test_get_users_empty_file() {
		let mut config = load_config();
		let csv_content = indoc! {r#"
          first_name,last_name,username,email,ldap_id,phone,enabled,needs_email_verification,needs_phone_verification
        "#};
		let _file = prepare_temp_csv_file(&mut config, csv_content);

		let source_csv = SourceCsv::new(&config).expect("Failed to create SourceCsv");

		let result = source_csv.read_csv();
		assert!(result.is_ok(), "Failed to get users: {:?}", result);

		let users = result.expect("Failed to get users");
		assert_eq!(users.len(), 0, "Expected empty user list");
	}

	#[test]
	fn test_get_users_invalid_file() {
		let mut config = load_config();
		if let Some(source_csv) = config.source_csv.as_mut() {
			source_csv.file_path = PathBuf::from("invalid_path.csv");
		}

		let source_csv = SourceCsv::new(&config).expect("Failed to create SourceCsv");

		let result = source_csv.read_csv();
		let error = result.expect_err("Expected error for invalid CSV data");
		assert!(
			error.to_string().contains("Failed to open CSV file"),
			"Unexpected error message: {:?}",
			error
		);
	}

	#[test]
	fn test_get_users_invalid_headers() {
		let mut config = load_config();
		let csv_content = indoc! {r#"
          first_name,last_name
          John,Doe
        "#};
		let _file = prepare_temp_csv_file(&mut config, csv_content);

		let source_csv = SourceCsv::new(&config).expect("Failed to create SourceCsv");

		let result = source_csv.read_csv();
		let error = result.expect_err("Expected error for invalid CSV data");
		assert!(
			error.to_string().contains("Failed to deserialize CSV data"),
			"Unexpected error message: {:?}",
			error
		);
	}

	#[test]
	fn test_get_users_invalid_content() {
		let mut config = load_config();
		let csv_content = indoc! {r#"
          first_name,last_name,username,email,ldap_id,phone,enabled,needs_email_verification,needs_phone_verification
          John,Doe
        "#};
		let _file = prepare_temp_csv_file(&mut config, csv_content);

		let source_csv = SourceCsv::new(&config).expect("Failed to create SourceCsv");

		let result = source_csv.read_csv();
		let error = result.expect_err("Expected error for invalid CSV data");
		assert!(
			error.to_string().contains("Failed to deserialize CSV data"),
			"Unexpected error message: {:?}",
			error
		);
	}
}
