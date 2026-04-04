use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SummarySection {
    Runs,
    Metrics,
    Curves,
    Matrix,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SummaryConfig {
    pub sections: Vec<SummarySection>,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            sections: vec![
                SummarySection::Runs,
                SummarySection::Metrics,
                SummarySection::Curves,
                SummarySection::Matrix,
            ],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub summary: SummaryConfig,
}

pub fn load_config(store_root: &Path) -> Config {
    let config_path = store_root.join("config.toml");
    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(config) = toml::from_str(&content) {
                return config;
            }
        }
    }
    Config::default()
}
