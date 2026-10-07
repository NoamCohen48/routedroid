//! `routedroid stop`: disconnect one phone and wait for it to be gone. The
//! exit code says how the connection ended.

use anyhow::{Result, bail};
use routedroid_ipc::{Client, DaemonError, Kind, Request, Response, label};

use super::answer;
use crate::exit;
use crate::output::print_json;

/// `phone` is a serial or a remembered name; `None` is the one connected.
pub async fn run(client: &Client, phone: Option<String>, json: bool) -> Result<i32> {
    let serial = match phone {
        Some(phone) => phone,
        None => only_connection(client).await?,
    };
    let request = Request::Stop { serial };
    let (serial, outcome) = answer!(client.call_ok(request).await?,
        Response::Stopped { serial, outcome } => (serial, outcome));
    if json {
        print_json(&outcome)?;
    } else {
        println!("{serial}: {outcome}");
    }
    Ok(exit::for_outcome(&outcome))
}

async fn only_connection(client: &Client) -> Result<String> {
    let connections = answer!(
        client.call_ok(Request::Status).await?,
        Response::Status { connections } => connections
    );
    match connections.as_slice() {
        [one] => Ok(one.serial.clone()),
        [] => bail!(usage("nothing is connected".into())),
        many => {
            let phones: Vec<String> = many
                .iter()
                .map(|c| label(&c.serial, c.name.as_deref()))
                .collect();
            bail!(usage(format!(
                "{} phones are connected ({}): say which one",
                many.len(),
                phones.join(", ")
            )))
        }
    }
}

/// Exits 2, like the daemon's own refusals.
fn usage(message: String) -> DaemonError {
    DaemonError {
        kind: Kind::Usage,
        message,
    }
}
