//! The phones this user has asked the daemon to remember, kept in
//! `~/.config/routedroid/phones.toml`: names to call them by, the options
//! they connect with, and whether plugging one in connects it. Written
//! whole on every change (to a temporary file, then renamed), so a crash
//! leaves the old list or the new one.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Context;
use routedroid_ipc::Phone;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::fault::{Fault, FaultExt, Kind, Result};

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default, rename = "phone")]
    phones: Vec<Phone>,
}

#[derive(Clone, Default)]
pub struct Phones {
    /// `None`: kept in memory only (tests).
    path: Option<Arc<PathBuf>>,
    list: Arc<Mutex<Vec<Phone>>>,
}

/// `$XDG_CONFIG_HOME/routedroid/phones.toml`, else under `~/.config`.
pub fn default_path() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    config.join("routedroid").join("phones.toml")
}

impl Phones {
    /// The list in `path`; a missing file is an empty list, and an
    /// unreadable one is logged and treated as empty (it is not overwritten
    /// until something is remembered).
    pub fn load(path: PathBuf) -> Self {
        let list = match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<File>(&text) {
                Ok(file) => file.phones,
                Err(error) => {
                    warn!(path = %path.display(), %error, "remembered phones unreadable; starting with none");
                    Vec::new()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                warn!(path = %path.display(), %error, "remembered phones unreadable; starting with none");
                Vec::new()
            }
        };
        Self {
            path: Some(Arc::new(path)),
            list: Arc::new(Mutex::new(list)),
        }
    }

    pub fn all(&self) -> Vec<Phone> {
        self.lock().clone()
    }

    /// The phone `key` names, by name or serial (a name is never another
    /// phone's serial, so they cannot disagree).
    pub fn find(&self, key: &str) -> Option<Phone> {
        find(&self.lock(), key)
    }

    /// The serial `key` means: a remembered name's phone, else `key` itself.
    pub fn serial(&self, key: &str) -> String {
        self.find(key).map_or_else(|| key.to_string(), |p| p.serial)
    }

    pub fn remember(&self, phone: Phone) -> Result<Phone> {
        let mut list = self.lock();
        check::check(&phone, &list)?;
        let mut next = list.clone();
        match next.iter_mut().find(|p| p.serial == phone.serial) {
            Some(slot) => *slot = phone.clone(),
            None => next.push(phone.clone()),
        }
        self.save(&next)?;
        *list = next;
        Ok(phone)
    }

    pub fn forget(&self, key: &str) -> Result<Phone> {
        let mut list = self.lock();
        let Some(phone) = find(&list, key) else {
            return Err(usage(format!("{key} is not a remembered phone")));
        };
        let next: Vec<Phone> = list
            .iter()
            .filter(|p| p.serial != phone.serial)
            .cloned()
            .collect();
        self.save(&next)?;
        *list = next;
        Ok(phone)
    }

    fn save(&self, phones: &[Phone]) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        write(path, phones)
            .with_context(|| format!("remember phones in {}", path.display()))
            .fault(Kind::Internal)
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Phone>> {
        self.list
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn find(list: &[Phone], key: &str) -> Option<Phone> {
    list.iter()
        .find(|p| p.name.as_deref() == Some(key) || p.serial == key)
        .cloned()
}

fn write(path: &Path, phones: &[Phone]) -> anyhow::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let text = toml::to_string(&File {
        phones: phones.to_vec(),
    })?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(
        &tmp,
        format!("# Remembered by routedroidd; `routedroid phones` lists them.\n{text}"),
    )?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
fn usage(message: String) -> Fault {
    Fault::msg(Kind::Usage, message)
}

mod check;

#[cfg(test)]
mod tests;
