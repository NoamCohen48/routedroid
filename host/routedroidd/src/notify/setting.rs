//! Whether notifications are shown: `routedroid notifications on|off`, kept
//! in `settings.toml` beside the remembered phones so it outlasts a restart.
//! A daemon started with `--notify false` shows none, whatever it says.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::fault::{Fault, FaultExt, Kind, Result};

#[derive(Debug, Serialize, Deserialize)]
struct File {
    #[serde(default = "yes")]
    notify: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone)]
pub struct Setting {
    /// `None`: kept in memory only (tests).
    path: Option<Arc<PathBuf>>,
    /// `false`: started with `--notify false`.
    allowed: bool,
    on: Arc<AtomicBool>,
}

impl Setting {
    /// The setting in `path`; missing or unreadable, notifications are on.
    pub fn load(path: PathBuf, allowed: bool) -> Self {
        let on = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str::<File>(&text).map_or_else(
                |error| {
                    warn!(path = %path.display(), %error, "settings unreadable; notifications on");
                    true
                },
                |file| file.notify,
            ),
            Err(_) => true,
        };
        Self {
            path: Some(Arc::new(path)),
            allowed,
            on: Arc::new(AtomicBool::new(on)),
        }
    }

    #[cfg(test)]
    pub fn in_memory(allowed: bool) -> Self {
        Self {
            path: None,
            allowed,
            on: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Not started with `--notify false`.
    pub fn allowed(&self) -> bool {
        self.allowed
    }

    pub fn on(&self) -> bool {
        self.allowed && self.on.load(Ordering::Relaxed)
    }

    pub fn set(&self, on: bool) -> Result<()> {
        if on && !self.allowed {
            return Err(Fault::msg(
                Kind::Usage,
                "this daemon was started with notifications off (--notify false or \
                 ROUTEDROID_NOTIFY=false)",
            ));
        }
        if let Some(path) = &self.path {
            write(path, on)
                .with_context(|| format!("save the setting in {}", path.display()))
                .fault(Kind::Internal)?;
        }
        self.on.store(on, Ordering::Relaxed);
        Ok(())
    }
}

fn write(path: &Path, notify: bool) -> anyhow::Result<()> {
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    let text = toml::to_string(&File { notify })?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(
        &tmp,
        format!("# Kept by routedroidd; `routedroid notifications` sets it.\n{text}"),
    )?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests;
