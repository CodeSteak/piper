use std::{path::{PathBuf, Path}, collections::HashMap, str::FromStr};
use serde::{Serialize, Deserialize};

use crate::tar_hash::TarHash;

#[derive(Clone)]
pub struct MetaStore {
    path : PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetaData {
    pub owner_token : String,
    pub delete_at_unix : u64,
    pub created_at_unix : u64,
    pub finished : bool,
}

impl MetaStore {
    pub fn new<P : AsRef<Path>>(path : P) -> Self {
        Self { path: path.as_ref().to_path_buf() }
    }

    pub fn get(&self, id : &TarHash) -> anyhow::Result<Option<MetaData>> {
        let path = self.path.join(&format!("{}.meta.json", id));
        if !path.exists() {
            return Ok(None);
        }

        let data = std::fs::read_to_string(path)?;
        let meta : MetaData = serde_json::from_str(&data)?;
        Ok(Some(meta))
    }

    pub fn set(&self, id : &TarHash, meta : &MetaData) -> anyhow::Result<()> {
        let path = self.path.join(&format!("{}.meta.json", id));
        let data = serde_json::to_string(meta)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    pub fn delete(&self, id : &TarHash) -> anyhow::Result<()> {
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
            if path.extension().unwrap_or_default() != "meta.json" {
                continue;
            }
            match path.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| TarHash::from_str(s).ok()) {
                    Some(id) => {
                        let data = std::fs::read_to_string(path)?;
                        let meta : MetaData = serde_json::from_str(&data)?;
                        map.insert(id, meta);
                    },
                    None => continue,
            }
        }
        Ok(map)
    }

}