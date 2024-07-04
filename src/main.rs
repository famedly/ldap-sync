use std::{process::ExitCode, str::FromStr};

use anyhow::Context;
use ldap_sync::{do_the_thing, Config};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> ExitCode {
	match read_the_config_and_do_the_thing().await {
		Ok(_) => ExitCode::SUCCESS,
		Err(e) => {
			tracing::error!("{}", e);
			ExitCode::FAILURE
		}
	}
}

async fn read_the_config_and_do_the_thing() -> anyhow::Result<()> {
	let config: Config = serde_yaml::from_slice(
		&tokio::fs::read(std::env::var("FAMEDLY_LDAP_SYNC_CONFIG").unwrap_or("config.yaml".into()))
			.await?,
	)?;
	let subscriber = tracing_subscriber::FmtSubscriber::builder()
		.with_max_level(
			config
				.log_level
				.as_ref()
				.map_or(Ok(LevelFilter::INFO), |s| LevelFilter::from_str(s))?,
		)
		.finish();
	tracing::subscriber::set_global_default(subscriber)
		.context("Setting default tracing subscriber failed")?;
	do_the_thing(config).await
}
