//! `routedroid doctor`: what stands between this host and a working phone.
//! Without `--repair` it only names the changes; with it, makes them.

use anyhow::Result;
use routedroid_ipc::{Check, CheckStatus, Client, Request, Response};
use serde::Serialize;

use super::answer;
use crate::exit;
use crate::output::print_json;

#[derive(Serialize)]
struct Report<'a> {
    checks: &'a [Check],
    done: &'a [String],
}

pub async fn run(client: &Client, repair: bool, json: bool) -> Result<i32> {
    let (checks, done) = answer!(
        client.call_ok(Request::Doctor { repair }).await?,
        Response::Doctor { checks, done } => (checks, done)
    );
    if json {
        print_json(&Report {
            checks: &checks,
            done: &done,
        })?;
    } else {
        print(&checks, &done, repair);
    }
    let failed = checks.iter().any(|c| c.status == CheckStatus::Fail);
    Ok(if failed { exit::PROBLEMS } else { exit::OK })
}

fn print(checks: &[Check], done: &[String], repair: bool) {
    for change in done {
        println!("repaired  {change}");
    }
    for check in checks {
        let status = match check.status {
            CheckStatus::Ok => "ok",
            CheckStatus::Warn => "warning",
            CheckStatus::Fail => "FAIL",
        };
        println!("{status:<9} {}: {}", check.name, check.detail);
        let verb = if repair { "not done" } else { "would" };
        for change in &check.repair {
            println!("          {verb}: {change}");
        }
    }
    let repairable = checks.iter().any(|c| !c.repair.is_empty());
    if repairable && !repair {
        println!("run `routedroid doctor --repair` to make these changes");
    }
}
