//! `sudo routedroid setup`: everything a first connection needs that only
//! root can do, asked for in plain words. It puts the user in group
//! `routedroid`, lets phones join the LAN through one interface in the
//! helper's policy (checked with the helper afterwards), and enables the
//! user's daemon. Run again, it changes only what is not so already.

mod ask;
mod block;
mod choose;
mod helper;
mod policy;
mod report;
mod system;

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use self::block::Block;
pub use self::report::Usage;
use self::system::{Daemon, GROUP};

#[derive(Debug, Args)]
pub struct SetupArgs {
    /// Who will connect phones; by default the user who ran sudo.
    #[arg(long)]
    pub user: Option<String>,
    /// The LAN interface phones join through; asked on a terminal when left out.
    #[arg(long)]
    pub lan_if: Option<String>,
    /// Phones lease their address from the LAN's DHCP server (the default
    /// unless --phone-addresses is given).
    #[arg(long)]
    pub dhcp: bool,
    /// A block of addresses phones may take, e.g. 192.168.1.200/29 (repeatable).
    #[arg(long = "phone-addresses", value_name = "CIDR")]
    pub phone_addresses: Vec<Block>,
    /// Take the default answers instead of asking.
    #[arg(long, short = 'y')]
    pub yes: bool,
    #[arg(long, hide = true, default_value = routedroid_helper_ipc::DEFAULT_SOCKET)]
    pub helper_socket: PathBuf,
    #[arg(long, hide = true, default_value = policy::PATH)]
    pub policy: PathBuf,
}

pub async fn run(args: SetupArgs) -> Result<i32> {
    report::exit(setup(args).await)
}

async fn setup(args: SetupArgs) -> Result<i32> {
    if !system::is_root() {
        return Err(Usage(
            "setup changes system files: run it with sudo: sudo routedroid setup".into(),
        )
        .into());
    }
    let user = system::target_user(args.user.clone(), std::env::var("SUDO_USER").ok())?;
    if !system::group_exists()? {
        return Err(Usage(format!(
            "there is no group {GROUP}: install Routedroid first (its package, or install.sh)"
        ))
        .into());
    }
    let mut done = Vec::new();
    let mut left = Vec::new();

    let joined = !system::in_group(&user)?;
    if joined {
        system::add_to_group(&user)?;
        done.push(format!("added {user} to group {GROUP}"));
    }
    // Some(false) also when they joined earlier but have not logged out since.
    let session = system::session_has_group(&user)?;
    if joined || session == Some(false) {
        left.push(format!(
            "log out completely and back in (or reboot), so {user}'s session has group {GROUP}"
        ));
    }

    let interfaces = helper::interfaces(&args.helper_socket).await?;
    let choice = if let Some(lan_if) = &args.lan_if {
        choose::from_flags(&interfaces, lan_if, args.dhcp, &args.phone_addresses)?
    } else if args.yes {
        choose::defaults(&interfaces)?
    } else if std::io::stdin().is_terminal() {
        ask::ask(
            &interfaces,
            &mut std::io::stdin().lock(),
            &mut std::io::stdout(),
        )?
    } else {
        return Err(Usage("no terminal to ask on: choose with --lan-if NAME (and --dhcp or --phone-addresses CIDR), or take the defaults with --yes".into()).into());
    };

    done.extend(policy::apply(&args.policy, &choice)?);
    let after = helper::interfaces(&args.helper_socket).await?;
    if let Some(why) = after
        .iter()
        .find(|link| link.name == choice.lan_if)
        .map_or(Some("it is gone"), |link| link.ineligible.as_deref())
    {
        eprintln!(
            "error: the helper still refuses phones on {}: {why}",
            choice.lan_if
        );
        return Ok(1);
    }

    match system::enable_daemon(&user, session) {
        Daemon::Enabled { started: true } => {
            done.push("enabled and started the daemon (routedroid.service)".into());
        }
        Daemon::Enabled { started: false } => {
            done.push("enabled the daemon; it starts when you log in".into());
        }
        Daemon::NoManager => left.push(format!(
            "as {user}, once logged in: systemctl --user enable --now routedroid"
        )),
        Daemon::AlreadyEnabled | Daemon::NoSystemd => {}
    }
    left.push("plug in a phone with USB debugging on, and run: routedroid start".into());
    report::summary(&user, &done, &left);
    Ok(0)
}

#[cfg(test)]
mod tests;
