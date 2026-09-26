//! Listing the journal directory and reading headers.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::record::Header;

pub const SUFFIX: &str = "journal";
pub const TMP_SUFFIX: &str = "journal.tmp";

/// Paths in `dir` ending in `.<suffix>`, sorted. A missing directory is empty.
pub fn list(dir: &Path, suffix: &str) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("read {}", dir.display())),
    };
    let mut out = Vec::new();
    for entry in entries {
        let path = entry.with_context(|| format!("read {}", dir.display()))?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.strip_suffix(suffix).is_some_and(|stem| stem.ends_with('.') && stem.len() > 1) {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// Only the first line: headers are written once, before the file is visible.
pub fn read_header(path: &Path) -> Result<Header> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut line = String::new();
    BufReader::new(file).read_line(&mut line).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&line).with_context(|| format!("header of {}", path.display()))
}
