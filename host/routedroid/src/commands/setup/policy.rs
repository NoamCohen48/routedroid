//! The helper's policy file, as `setup` edits it: read what is there, allow
//! one interface (keeping every other entry), and write it back whole. The
//! old file is kept as `.bak`; comments in it are not carried over.

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use super::choose::Choice;

pub const PATH: &str = "/etc/routedroid/helper.toml";

const HEADER: &str = "\
# Which LAN interfaces may carry phones, and which addresses phones may take
# there: requested ones inside `phone_addresses`, and with `dhcp = true` an
# address leased from the LAN's DHCP server (inside `phone_addresses` too, if
# any are listed). Owned by root, not writable by group or others, or the
# helper refuses every session. `routedroid interfaces` shows what it allows.
# Written by `sudo routedroid setup`; edit freely.
";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyFile {
    #[serde(default, rename = "interface")]
    interfaces: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    #[serde(default)]
    phone_addresses: Vec<String>,
    #[serde(default)]
    dhcp: bool,
}

impl PolicyFile {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(toml::from_str(text)?)
    }

    /// Allow what `choice` says on its interface, in place of any entry it
    /// had; whether that changed anything.
    pub fn allow(&mut self, choice: &Choice) -> bool {
        let entry = Entry {
            name: choice.lan_if.clone(),
            phone_addresses: choice.blocks.iter().map(ToString::to_string).collect(),
            dhcp: choice.dhcp,
        };
        match self.interfaces.iter_mut().find(|e| e.name == entry.name) {
            Some(old) if *old == entry => false,
            Some(old) => {
                *old = entry;
                true
            }
            None => {
                self.interfaces.push(entry);
                true
            }
        }
    }

    pub fn render(&self) -> String {
        let mut text = HEADER.to_string();
        for entry in &self.interfaces {
            text += &format!("\n[[interface]]\nname = {}\n", quote(&entry.name));
            if !entry.phone_addresses.is_empty() {
                let blocks: Vec<String> = entry.phone_addresses.iter().map(|b| quote(b)).collect();
                text += &format!("phone_addresses = [{}]\n", blocks.join(", "));
            }
            text += &format!("dhcp = {}\n", entry.dhcp);
        }
        text
    }
}

/// Allow `choice` in the file at `path`; what was done, if anything.
pub fn apply(path: &Path, choice: &Choice) -> Result<Option<String>> {
    let shown = path.display();
    let old = read(path)?;
    let mut file = match &old {
        Some(text) => PolicyFile::parse(text).with_context(|| {
            format!("{shown} cannot be read; fix it, or move it away and run setup again")
        })?,
        None => PolicyFile::default(),
    };
    if !file.allow(choice) {
        return Ok(None);
    }
    write(path, &file.render(), old.as_deref())?;
    let backup = match old {
        Some(_) => format!(" (the old one is {shown}.bak)"),
        None => String::new(),
    };
    let (lan_if, addresses) = (&choice.lan_if, choice.addresses());
    Ok(Some(format!(
        "let phones join through {lan_if} ({addresses}) in {shown}{backup}"
    )))
}

/// A TOML basic string.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The file's text, or `None` when there is none yet.
pub fn read(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

/// Replace the file at once (a reader sees the old policy or the new one),
/// keeping the old one as `.bak`. Mode 0644, owned by whoever runs this
/// (root): the helper refuses a policy others could write.
pub fn write(path: &Path, text: &str, old: Option<&str>) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    if let Some(old) = old {
        let backup = sibling(path, "bak");
        fs::write(&backup, old).with_context(|| format!("write {}", backup.display()))?;
    }
    let tmp = sibling(path, "tmp");
    let mut file = fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
    file.set_permissions(fs::Permissions::from_mode(0o644))?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::rename(&tmp, path).with_context(|| format!("replace {}", path.display()))
}

/// `helper.toml` -> `helper.toml.EXT`.
fn sibling(path: &Path, ext: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{ext}"));
    PathBuf::from(name)
}

#[cfg(test)]
mod tests;
