use serde::{Deserialize, Serialize};
use std::{fmt::Display, path::PathBuf};

#[derive(Deserialize, Serialize, Debug, Default)]
pub struct Config {
    pub host: Option<String>,
    pub token: Option<String>,
    pub protocol: Option<Protocol>,
    pub history_file: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Default)]
pub enum Protocol {
    #[default]
    Https,
    Http,
}

impl Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::Https => write!(f, "https"),
            Protocol::Http => write!(f, "http"),
        }
    }
}

pub fn config_path() -> PathBuf {
    let mut path = dirs::config_dir().expect("Could not find config directory");
    path.push("toc");
    path.push("config.toml");
    path
}

#[allow(unused)]
pub fn history_path() -> PathBuf {
    let mut path = dirs::config_dir().expect("Could not find config directory");
    path.push("toc");
    path.push("history");
    path
}

impl Config {
    pub fn load(path: &Option<PathBuf>) -> anyhow::Result<Self> {
        let path = path.clone().unwrap_or_else(config_path);
        if !path.exists() {
            return Ok(Self::default());
        }

        let config = std::fs::read_to_string(path)?;
        let config = toml::from_str(&config)?;
        Ok(config)
    }

    pub fn save(&self, path: &Option<PathBuf>) -> anyhow::Result<PathBuf> {
        let path = path.clone().unwrap_or_else(config_path);
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let config = toml::to_string_pretty(&self)?;
        std::fs::write(&path, config)?;
        Ok(path)
    }
}
