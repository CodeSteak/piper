use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::tar_hash::TarHash;

#[derive(Clone)]
pub struct MetaStore {
    path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetaData {
    pub owner: String,
    pub delete_at_unix: u64,
    pub created_at_unix: u64,
    pub allow_write: bool,
    pub allow_rewrite: bool,
    pub finished: bool,
}

impl MetaStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn get(&self, id: &TarHash) -> anyhow::Result<Option<MetaData>> {
        let path = self.path.join(&format!("{}.meta.json", id));
        if !path.exists() {
            return Ok(None);
        }

        let data = std::fs::read_to_string(path)?;
        let meta: MetaData = serde_json::from_str(&data)?;
        Ok(Some(meta))
    }

    pub fn file_path(&self, id: &TarHash) -> PathBuf {
        self.path.join(&format!("{}.tar.age", id))
    }

    pub fn set(&self, id: &TarHash, meta: &MetaData) -> anyhow::Result<()> {
        let path = self.path.join(&format!("{}.meta.json", id));
        let data = serde_json::to_string(meta)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    pub fn delete(&self, id: &TarHash) -> anyhow::Result<()> {
        let path = self.path.join(&format!("{}.meta.json", id));
        if !path.exists() {
            return Ok(());
        }
        std::fs::remove_file(path)?;
        Ok(())
    }

    pub fn list(&self) -> anyhow::Result<HashMap<TarHash, MetaData>> {
        let mut map = HashMap::new();
        for entry in std::fs::read_dir(&self.path)? {
            let entry = entry?;
            let path = entry.path();

            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default();
            if !file_name.ends_with(".meta.json") {
                continue;
            }
            match TarHash::from_str(
                file_name
                    .split_once(".")
                    .expect("file has meta.json but no '.'.")
                    .0,
            )
            .ok()
            {
                Some(id) => {
                    let data = std::fs::read_to_string(path)?;
                    let meta: MetaData = serde_json::from_str(&data)?;
                    map.insert(id, meta);
                }
                None => continue,
            }
        }
        Ok(map)
    }
}
