//! `routedroid completions SHELL` and the hidden `routedroid manpages DIR`:
//! both made from the command-line definitions themselves, so they cannot
//! fall behind them. Neither needs the daemon.

use std::path::Path;

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::Shell;

use crate::cli::Cli;

/// The completion script for `shell`, on stdout.
pub fn completions(shell: Shell) -> i32 {
    clap_complete::generate(
        shell,
        &mut Cli::command(),
        "routedroid",
        &mut std::io::stdout(),
    );
    0
}

/// `routedroid.1` and one page per subcommand (`routedroid-start.1`, ...) in `dir`.
pub fn manpages(dir: &Path) -> Result<i32> {
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let mut command = Cli::command();
    command.build();
    write(dir, command.clone(), "routedroid")?;
    for sub in command.get_subcommands() {
        if sub.is_hide_set() || sub.get_name() == "help" {
            continue;
        }
        let usage = sub
            .clone()
            .bin_name(format!("routedroid {}", sub.get_name()));
        write(dir, usage, &format!("routedroid-{}", sub.get_name()))?;
    }
    Ok(0)
}

/// `command`'s page as `dir/title.1`.
fn write(dir: &Path, command: clap::Command, title: &str) -> Result<()> {
    let path = dir.join(format!("{title}.1"));
    let mut page = Vec::new();
    clap_mangen::Man::new(command)
        .title(title)
        .render(&mut page)?;
    std::fs::write(&path, page).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests;
