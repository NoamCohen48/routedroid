//! `routedroid phones`, `remember` and `forget`: the phones the daemon
//! knows by name, connects with their own options, and (unless told not
//! to) connects whenever they are plugged in.

use anyhow::Result;
use clap::Args;
use routedroid_ipc::{Client, Phone, Request, Response};

use super::answer;
use super::options::Options;
use crate::output::{header, print_json, print_table};

#[derive(Debug, Args)]
pub struct RememberArgs {
    /// The phone: a serial, or the name it is remembered by.
    pub phone: String,
    /// Call it NAME from now on.
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
    /// Connect it only when asked, not whenever it is plugged in.
    #[arg(long)]
    pub no_auto: bool,
    #[command(flatten)]
    pub options: Options,
}

pub async fn list(client: &Client, json: bool) -> Result<i32> {
    let phones =
        answer!(client.call_ok(Request::Phones).await?, Response::Phones { phones } => phones);
    if json {
        print_json(&phones)?;
    } else if phones.is_empty() {
        println!("no phones remembered (`routedroid start --remember` remembers one)");
    } else {
        let mut rows = vec![header(&[
            "NAME",
            "SERIAL",
            "WHEN_PLUGGED_IN",
            "LAN_IF",
            "ADDRESS",
        ])];
        rows.extend(phones.iter().map(row));
        print_table(&rows);
    }
    Ok(0)
}

fn row(phone: &Phone) -> Vec<String> {
    let dash = || "-".to_string();
    vec![
        phone.name.clone().unwrap_or_else(dash),
        phone.serial.clone(),
        if phone.auto {
            "connect"
        } else {
            "wait to be asked"
        }
        .into(),
        phone.lan_if.clone().unwrap_or_else(dash),
        phone
            .phone_ip
            .map_or_else(|| "DHCP".into(), |ip| ip.to_string()),
    ]
}

pub async fn run_remember(client: &Client, args: RememberArgs, json: bool) -> Result<i32> {
    let auto = Some(!args.no_auto);
    let phone = remember(client, &args.phone, args.name, auto, &args.options).await?;
    if json {
        print_json(&phone)?;
    } else {
        let when = if phone.auto {
            "connects whenever it is plugged in"
        } else {
            "connects when asked"
        };
        println!("remembered {}: it {when}", phone.label());
    }
    Ok(0)
}

/// Remember `key` (a serial or a remembered name) with `options` over what
/// is remembered already; `name` and `auto` change only when given.
pub async fn remember(
    client: &Client,
    key: &str,
    name: Option<String>,
    auto: Option<bool>,
    options: &Options,
) -> Result<Phone> {
    let phones =
        answer!(client.call_ok(Request::Phones).await?, Response::Phones { phones } => phones);
    let known = phones
        .into_iter()
        .find(|p| p.serial == key || p.name.as_deref() == Some(key));
    let mut phone = options.onto(known.unwrap_or_else(|| Phone {
        serial: key.to_string(),
        ..Phone::default()
    }));
    phone.name = name.or(phone.name);
    phone.auto = auto.unwrap_or(phone.auto);
    let response = client.call_ok(Request::Remember(phone)).await?;
    Ok(answer!(response, Response::Remembered { phone } => phone))
}

pub async fn forget(client: &Client, key: &str, json: bool) -> Result<i32> {
    let request = Request::Forget {
        phone: key.to_string(),
    };
    let phone = answer!(client.call_ok(request).await?, Response::Forgotten { phone } => phone);
    if json {
        print_json(&phone)?;
    } else {
        println!("forgot {}", phone.label());
    }
    Ok(0)
}
