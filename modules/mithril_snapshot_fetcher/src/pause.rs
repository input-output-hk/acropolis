use std::time::Instant;

use acropolis_common::BlockInfo;
use config::Config;
use tracing::{error, info};

#[derive(Debug, Clone, PartialEq)]
pub enum PauseType {
    NoPause,
    Epoch {
        number: u64,
        start_time: std::time::Instant,
    },
    Block {
        number: u64,
        start_time: std::time::Instant,
    },
}

impl PauseType {
    pub fn from_config(config: &Config, default_pause: (&str, PauseType)) -> Option<Self> {
        let pause_str = config.get_string(default_pause.0).ok()?;

        if pause_str.eq_ignore_ascii_case("none") {
            return Some(PauseType::NoPause);
        }

        let parts: Vec<&str> = pause_str.split(':').collect();

        if parts.len() != 2 {
            error!(
                "Invalid pause format: {}. Expected format: 'type:value' (e.g., 'epoch:214', 'block:1200')",
                pause_str
            );
            return None;
        }

        let pause_type = parts[0].trim();
        let value = parts[1].trim().parse::<u64>().ok()?;
        let start_time = Instant::now();

        match pause_type {
            "epoch" => {
                info!("Pausing enabled at epoch {value}");
                Some(PauseType::Epoch {
                    number: value,
                    start_time,
                })
            }
            "block" => {
                info!("Pausing enabled at block {value}");
                Some(PauseType::Block {
                    number: value,
                    start_time,
                })
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
            PauseType::Epoch { number, .. } => block_info.new_epoch && block_info.epoch == *number,
            PauseType::Block { number, .. } => block_info.number == *number,
            PauseType::NoPause => false,
        }
    }

    pub fn next(&mut self) {
        match self {
            PauseType::Epoch { number, start_time } => {
                *number += 1;
                *start_time = Instant::now();
            }
            PauseType::Block { number, start_time } => {
                *number += 1;
                *start_time = Instant::now();
            }
            PauseType::NoPause => {}
        }
    }

    pub fn get_description(&self) -> String {
        match self {
            PauseType::Epoch { number, start_time } => {
                format!("Epoch {number} (started {:?} ago)", start_time.elapsed())
            }
            PauseType::Block { number, start_time } => {
                format!("Block {number} (started {:?} ago)", start_time.elapsed())
            }
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
        match pause_type {
            Some(PauseType::Epoch { number, .. }) => assert_eq!(number, 100),
            _ => panic!("Expected Some(PauseType::Epoch {{ number: 100, .. }})"),
        }
    }

    #[test]
    fn test_pause_type_from_config_block() {
        let config = Config::builder().set_override("pause", "block:100").unwrap().build().unwrap();
        let pause_type = PauseType::from_config(&config, DEFAULT_PAUSE);
        match pause_type {
            Some(PauseType::Block { number, .. }) => assert_eq!(number, 100),
            _ => panic!("Expected Some(PauseType::Block {{ number: 100, .. }})"),
        }
    }

    #[test]
    fn test_pause_type_from_config_invalid() {
        let config = Config::builder().set_override("pause", "invalid").unwrap().build().unwrap();
        let pause_type = PauseType::from_config(&config, DEFAULT_PAUSE);
        assert_eq!(pause_type, None);
    }
}
