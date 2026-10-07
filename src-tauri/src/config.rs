//! Saved connection profiles, persisted as JSON in the app config directory.
//! Passwords are never stored here; see [`crate::secrets`].

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Empty when creating a new profile; assigned on save.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AuthMethod {
    Password,
    PublicKey { key_path: String },
    Agent,
}

pub struct ProfileStore {
    path: PathBuf,
    profiles: Mutex<Vec<Profile>>,
}

impl ProfileStore {
    pub fn load(path: PathBuf) -> Result<Self> {
        let profiles = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self { path, profiles: Mutex::new(profiles) })
    }

    pub fn list(&self) -> Vec<Profile> {
        self.profiles.lock().unwrap().clone()
    }

    pub fn get(&self, id: &str) -> Result<Profile> {
        self.profiles
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| Error::ProfileNotFound(id.to_owned()))
    }

    /// Inserts or updates a profile and returns it with its id filled in.
    pub fn save(&self, mut profile: Profile) -> Result<Profile> {
        profile.name = profile.name.trim().to_owned();
        profile.host = profile.host.trim().to_owned();
        profile.username = profile.username.trim().to_owned();
        if profile.host.is_empty() || profile.username.is_empty() || profile.port == 0 {
            return Err(Error::Invalid("主机、端口和用户名不能为空".into()));
        }
        if profile.name.is_empty() {
            profile.name = format!("{}@{}", profile.username, profile.host);
        }

        let mut profiles = self.profiles.lock().unwrap();
        let mut updated = profiles.clone();
        match updated.iter_mut().find(|p| !profile.id.is_empty() && p.id == profile.id) {
            Some(existing) => *existing = profile.clone(),
            None => {
                profile.id = uuid::Uuid::new_v4().to_string();
                updated.push(profile.clone());
            }
        }
        self.persist(&updated)?;
        *profiles = updated;
        Ok(profile)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let mut profiles = self.profiles.lock().unwrap();
        let updated: Vec<_> = profiles.iter().filter(|p| p.id != id).cloned().collect();
        self.persist(&updated)?;
        *profiles = updated;
        Ok(())
    }

    fn persist(&self, profiles: &[Profile]) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        // Write-then-rename so a crash never leaves a truncated file behind.
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(profiles)?)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
