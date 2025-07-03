use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    timestamp: DateTime<Utc>,
    data: Value,
}

#[derive(Clone)]
pub struct Cache {
    cache_dir: PathBuf,
    expiry_hours: i64,
}

impl Cache {
    pub fn new(cache_dir: &str, expiry_hours: i64) -> Self {
        let cache_dir = PathBuf::from(cache_dir);
        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir).ok();
        }

        Self {
            cache_dir,
            expiry_hours,
        }
    }

    fn get_cache_key(key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        hex::encode(hasher.finalize())
    }

    fn get_cache_path(&self, key: &str) -> PathBuf {
        let cache_key = Self::get_cache_key(key);
        self.cache_dir.join(format!("{cache_key}.json"))
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        let cache_path = self.get_cache_path(key);

        if !cache_path.exists() {
            return None;
        }

        let contents = fs::read_to_string(&cache_path).ok()?;
        let entry: CacheEntry = serde_json::from_str(&contents).ok()?;

        // Check if cache is expired
        let age = Utc::now() - entry.timestamp;
        if age > Duration::hours(self.expiry_hours) {
            fs::remove_file(&cache_path).ok();
            return None;
        }

        Some(entry.data)
    }

    pub fn set(&self, key: &str, value: &Value) -> Result<()> {
        let cache_path = self.get_cache_path(key);

        let entry = CacheEntry {
            timestamp: Utc::now(),
            data: value.clone(),
        };

        let contents = serde_json::to_string_pretty(&entry)?;
        fs::write(cache_path, contents)?;
        Ok(())
    }
}
