//! Endpoint -> Famedly Zitadel sync tool.

use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;
use url::Url;

use crate::{config::FeatureFlags, Config, FeatureFlag};

/// UKT Source
pub struct SourceUkt {
	/// UKT Source configuration
	source_ukt_config: SourceUktConfig,
	/// Optional set of features
	feature_flags: FeatureFlags,
	/// Reqwest client
	client: Client,
}

impl SourceUkt {
	/// Create a new SourceUkt instance
	pub fn new(config: &Config) -> Result<Self> {
		let client = Client::new();
		Ok(Self {
			source_ukt_config: match config.source_ukt.clone() {
				Some(source_ukt) => source_ukt,
				None => bail!("Endpoint configuration is missing"),
			},
			feature_flags: config.feature_flags.clone(),
			client,
		})
	}

	/// Get list of user emails that have been removed
	pub async fn get_removed_user_emails(&self) -> Result<Vec<String>> {
		if self.feature_flags.is_enabled(FeatureFlag::DryRun) {
			tracing::warn!("Not fetching during a dry run");
			return Ok(vec![]);
		}

		let oauth2_token = self.get_oauth2_token().await?;
		let email_list = self.fetch_list(oauth2_token).await?;

		Ok(email_list)
	}

	/// Get the OAuth2 token
	async fn get_oauth2_token(&self) -> Result<OAuth2Token> {
		let mut params = HashMap::new();
		params.insert("grant_type", &self.source_ukt_config.grant_type);
		params.insert("scope", &self.source_ukt_config.scope);
		params.insert("client_id", &self.source_ukt_config.client_id);
		params.insert("client_secret", &self.source_ukt_config.client_secret);

		let response = self
			.client
			.post(self.source_ukt_config.oauth2_url.clone())
			.form(&params)
			.send()
			.await?;

		if response.status() != reqwest::StatusCode::OK {
			anyhow::bail!("Error in response: {}", response.status())
		}

		let response: serde_json::Value = response.json().await?;

		if let Some(error) = response.get("error") {
			anyhow::bail!("Error in response: {}", error)
		}

		let oauth2_token: OAuth2Token = serde_json::from_value(response)
			.context("Failed to deserialize oAuth2 token response")?;

		Ok(oauth2_token)
	}

	/// Fetch the list of users
	async fn fetch_list(&self, oauth2_token: OAuth2Token) -> Result<EmailList> {
		let current_date = Utc::now().format("%Y%m%d").to_string();

		let response = self
			.client
			.get(self.source_ukt_config.endpoint_url.clone())
			.query(&[("date", &current_date)])
			.bearer_auth(oauth2_token.access_token)
			.header("x-participant-token", oauth2_token.id_token)
			.send()
			.await?;

		if response.status() != reqwest::StatusCode::OK {
			anyhow::bail!("Error in response: {}", response.status())
		}

		let response: serde_json::Value = response.json().await?;

		if let Some(error) = response.get("error") {
			anyhow::bail!("Error in response: {}", error)
		}

		let email_list: EmailList = serde_json::from_value(response)
			.context("Failed to deserialize email list response")?;

		Ok(email_list)
	}
}

/// List of emails
type EmailList = Vec<String>;

/// OAuth2 token response
#[derive(Debug, Deserialize)]
struct OAuth2Token {
	/// Access token
	access_token: String,
	/// ID token
	id_token: String,
}

/// Configuration to get a list of users from an endpoint
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SourceUktConfig {
	/// The URL of the endpoint
	pub endpoint_url: Url,
	/// The OAuth2 URL for the endpoint
	pub oauth2_url: Url,
	/// The API client ID for the endpoint
	pub client_id: String,
	/// The API client secret for the endpoint
	pub client_secret: String,
	/// The scope for the endpoint
	pub scope: String,
	/// The grant type for the endpoint
	pub grant_type: String,
}

// Run these tests with
// RUST_TEST_THREADS=1 cargo test --lib
#[cfg(test)]
mod tests {
	#![allow(clippy::expect_used, clippy::unwrap_used)]

	use indoc::indoc;
	use reqwest::StatusCode;
	use url::Url;
	use wiremock::{
		matchers::{body_string_contains, header, method, path, query_param},
		Mock, MockServer, ResponseTemplate,
	};

	use super::*;

	const ENDPOINT_PATH: &str = "/usersync4chat/maillist";
	const OATH2_PATH: &str = "/token";

	const EXAMPLE_CONFIG: &str = indoc! {r#"
        source_ukt:
          endpoint_url: https://api.test.ukt.connext.com/usersync4chat/maillist
          oauth2_url: https://api.test.ukt.connext.com/token
          client_id: mock_client_id
          client_secret: mock_client_secret
          scope: "openid read-maillist"
          grant_type: client_credentials

        zitadel_config:
          url: http://localhost:8080
          key_file: tests/environment/zitadel/service-user.json
          organization_id: 1
          project_id: 1
          idp_id: 1

        feature_flags: []
        cache_path: ./test
	"#};

	fn full_config_example() -> Config {
		serde_yaml::from_str(EXAMPLE_CONFIG).expect("invalid config")
	}

	#[tokio::test]
	async fn test_get_oauth2_token() {
		let mock_server = MockServer::start().await;
		prepare_oauth2_mock(&mock_server).await;

		// Use the mock server URL in the config
		let mut config = full_config_example();
		config
			.source_ukt
			.as_mut()
			.map(|source_ukt| {
				source_ukt.oauth2_url = get_mock_server_url(&mock_server, OATH2_PATH)
					.expect("Failed to get mock server URL");
			})
			.expect("SourceUkt configuration is missing");

		let source_ukt = SourceUkt::new(&config).expect("Failed to create SourceUkt");

		let result = source_ukt.get_oauth2_token().await;
		assert!(result.is_ok(), "Failed to get OAuth2 token: {:?}", result);
	}

	#[tokio::test]
	async fn test_fetch_list() {
		let mock_server = MockServer::start().await;
		prepare_oauth2_mock(&mock_server).await;
		prepare_endpoint_mock(&mock_server).await;

		let mut config = full_config_example();
		config
			.source_ukt
			.as_mut()
			.map(|source_ukt| {
				source_ukt.oauth2_url = get_mock_server_url(&mock_server, OATH2_PATH)
					.expect("Failed to get mock server URL");
				source_ukt.endpoint_url = get_mock_server_url(&mock_server, ENDPOINT_PATH)
					.expect("Failed to get mock server URL");
			})
			.expect("SourceUkt configuration is missing");

		let source_ukt = SourceUkt::new(&config).expect("Failed to create SourceUkt");

		let oauth2_token = source_ukt.get_oauth2_token().await.expect("Failed to get access token");

		let result = source_ukt.fetch_list(oauth2_token).await;
		assert!(result.is_ok(), "Failed to fetch email list: {:?}", result);

		let email_list = result.expect("Failed to get email list");
		assert_eq!(email_list.len(), 2, "Unexpected number of emails");
		assert_eq!(email_list[0], "first_email@example.com", "Unexpected email at index 0");
		assert_eq!(email_list[1], "second_email@example.com", "Unexpected email at index 1");
	}

	#[tokio::test]
	async fn test_fetch_list_incorrect_verification() {
		let mock_server = MockServer::start().await;
		prepare_endpoint_mock(&mock_server).await;

		let mut config = full_config_example();
		config
			.source_ukt
			.as_mut()
			.map(|source_ukt| {
				source_ukt.endpoint_url = get_mock_server_url(&mock_server, ENDPOINT_PATH)
					.expect("Failed to get mock server URL");
			})
			.expect("SourceUkt configuration is missing");

		let source_ukt = SourceUkt::new(&config).expect("Failed to create SourceUkt");

		let incorrect_oauth2_token = OAuth2Token {
			access_token: "wrong_token".to_owned(),
			id_token: "wrong_id_token".to_owned(),
		};

		let result = source_ukt.fetch_list(incorrect_oauth2_token).await;
		assert!(result.is_err(), "Didn't expect to fetch email list: {:?}", result);
	}

	#[tokio::test]
	#[ignore]
	/// Connects to the real URL in config to get the OAuth2 token
	async fn real_test_get_oauth2_token() {
		let config = full_config_example();

		let source_ukt = SourceUkt::new(&config).expect("Failed to create SourceUkt");

		let result = source_ukt.get_oauth2_token().await;
		// println!("{:?}", result);
		assert!(result.is_ok());
	}

	#[tokio::test]
	#[ignore]
	/// Connects to the real URL in config to get the email list
	async fn real_test_fetch_list() {
		let config = full_config_example();

		let source_ukt = SourceUkt::new(&config).expect("Failed to create SourceUkt");

		let oauth2_token = source_ukt.get_oauth2_token().await.expect("Failed to get access token");

		let result = source_ukt.fetch_list(oauth2_token).await;
		// println!("{:?}", result);
		assert!(result.is_ok());
	}

	fn get_mock_server_url(mock_server: &MockServer, path: &str) -> Result<Url> {
		let url_with_endpoint = format!("{}{}", mock_server.uri(), path);
		Url::parse(&url_with_endpoint)
			.map_err(|error| anyhow::anyhow!("Failed to parse URL: {}", error))
	}

	async fn prepare_oauth2_mock(mock_server: &MockServer) {
		Mock::given(method("POST"))
			.and(path(OATH2_PATH))
			.and(body_string_contains("grant_type=client_credentials"))
			.and(body_string_contains("scope=openid+read-maillist"))
			.and(body_string_contains("client_id=mock_client_id"))
			.and(body_string_contains("client_secret=mock_client_secret"))
			.respond_with(ResponseTemplate::new(StatusCode::OK).set_body_string(
				r#"{
                "access_token": "mock_access_token",
                "id_token": "mock_id_token",
                "token_type": "Bearer",
                "scope": "openid read-maillist",
                "expires_in": 3600
            }"#,
			))
			.up_to_n_times(1)
			.mount(mock_server)
			.await;
	}

	async fn prepare_endpoint_mock(mock_server: &MockServer) {
		let current_date = Utc::now().format("%Y%m%d").to_string();

		Mock::given(method("GET"))
			.and(path(ENDPOINT_PATH))
			.and(query_param("date", &current_date))
			.and(header("x-participant-token", "mock_id_token"))
			.and(header("Authorization", "Bearer mock_access_token"))
			.respond_with(ResponseTemplate::new(StatusCode::OK).set_body_string(
				r#"[
          "first_email@example.com",
          "second_email@example.com"
        ]"#,
			))
			.up_to_n_times(1)
			.mount(mock_server)
			.await;
	}
}
