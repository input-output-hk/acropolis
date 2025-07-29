use acropolis_common::BlockInfo;
use config::Config;
use tracing::{error, info};

#[derive(Debug, Clone, PartialEq)]
pub enum PauseType {
    NoPause,
    Epoch(u64),
    Block(u64),
}

impl PauseType {
    pub fn from_config(config: &Config, default_pause: (&str, PauseType)) -> Option<Self> {
        let pause_str = config.get_string(default_pause.0).ok()?;

        if pause_str.eq_ignore_ascii_case("none") {
            return Some(PauseType::NoPause);
        }

        let parts: Vec<&str> = pause_str.split(':').collect();

        if parts.len() != 2 {
            error!("Invalid pause format: {}. Expected format: 'type:value' (e.g., 'epoch:214', 'block:1200')", pause_str);
            return None;
        }

        let pause_type = parts[0].trim();
        let value = parts[1].trim().parse::<u64>().ok()?;

        match pause_type {
            "epoch" => {
                info!("Pausing enabled at epoch {value}");
                Some(PauseType::Epoch(value))
            }
            "block" => {
                info!("Pausing enabled at block {value}");
                Some(PauseType::Block(value))
            }
            _ => {
                error!(
                    "Unknown pause type: {}. Supported types: epoch, block",
                    pause_type
                );
                None
            }
        }
    }

    pub fn should_pause(&self, block_info: &BlockInfo) -> bool {
        match self {
            PauseType::Epoch(target_epoch) => {
                if block_info.new_epoch {
                    return block_info.epoch == *target_epoch;
                }
                return false;
            }
            PauseType::Block(target_block) => block_info.number == *target_block,
            PauseType::NoPause => false,
        }
    }

    pub fn get_next(&self) -> Self {
        match self {
            PauseType::Epoch(target_epoch) => PauseType::Epoch(target_epoch + 1),
            PauseType::Block(target_block) => PauseType::Block(target_block + 1),
            PauseType::NoPause => PauseType::NoPause,
        }
    }

    pub fn get_description(&self) -> String {
        match self {
            PauseType::Epoch(target_epoch) => format!("Epoch {target_epoch}"),
            PauseType::Block(target_block) => format!("Block {target_block}"),
            PauseType::NoPause => "No pause".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const DEFAULT_PAUSE: (&str, PauseType) = ("pause", PauseType::NoPause);

    #[test]
    fn test_pause_type_from_config_epoch() {
        let config = Config::builder().set_override("pause", "epoch:100").unwrap().build().unwrap();
        let pause_type = PauseType::from_config(&config, DEFAULT_PAUSE);
        assert_eq!(pause_type, Some(PauseType::Epoch(100)));
    }

    #[test]
    fn test_pause_type_from_config_block() {
        let config = Config::builder().set_override("pause", "block:100").unwrap().build().unwrap();
        let pause_type = PauseType::from_config(&config, DEFAULT_PAUSE);
        assert_eq!(pause_type, Some(PauseType::Block(100)));
    }

    #[test]
    fn test_pause_type_from_config_invalid() {
        let config = Config::builder().set_override("pause", "invalid").unwrap().build().unwrap();
        let pause_type = PauseType::from_config(&config, DEFAULT_PAUSE);
        assert_eq!(pause_type, None);
    }
}
