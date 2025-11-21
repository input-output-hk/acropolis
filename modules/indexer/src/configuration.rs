use anyhow::Result;
use config::Config;

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct IndexerConfig {
    pub sync_command_topic: String,
}

impl IndexerConfig {
    pub fn try_load(config: &Config) -> Result<Self> {
        let full_config = Config::builder()
            .add_source(config::File::from_str(
                include_str!("../config.default.toml"),
                config::FileFormat::Toml,
            ))
            .add_source(config.clone())
            .build()?;
        Ok(full_config.try_deserialize()?)
    }
}
