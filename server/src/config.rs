use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    pub general: GeneralConfig,
    pub users: Vec<UserConfig>,
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Config> {
        let config = std::fs::read_to_string(path)?;
        let config = toml::from_str(&config)?;
        Ok(config)
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct GeneralConfig {
    #[serde(default = "default_servername")]
    pub hostname: String,
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    #[serde(default = "default_gc_interval_s")]
    pub gc_interval_s: u64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct UserConfig {
    pub username: String,
    pub token: String,
}

fn default_servername() -> String {
    "localhost".to_string()
}

fn default_listen() -> String {
    "[::1]:8000".to_string()
}

fn default_gc_interval_s() -> u64 {
    // 1h
    60 * 60
}

fn default_data_dir() -> String {
    "./data".to_string()
}
