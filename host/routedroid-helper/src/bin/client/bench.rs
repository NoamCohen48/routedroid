//! Relay benchmark: push ICMP echo requests from the phone's address into
//! the helper and count the host kernel's replies coming back out.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::icmp;
use crate::link::{Incoming, Link};
use crate::run::ClientArgs;

const WINDOW: u32 = 64;

/// Returns the number of echo replies seen before the deadline.
pub async fn run(
    link: &mut Link,
    args: &ClientArgs,
    phone_ip: Ipv4Addr,
    target: Ipv4Addr,
) -> Result<u32> {
    // Let the kernel finish bringing the TUN up before timing.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let id = (std::process::id() & 0xffff) as u16;
    let deadline = tokio::time::sleep(Duration::from_secs(5 + u64::from(args.bench) / 2000));
    tokio::pin!(deadline);
    let started = Instant::now();
    let (mut sent, mut replies) = (0u32, 0u32);
    // At most WINDOW requests in flight: the helper drops replies it cannot
    // queue towards us, so an unbounded burst would measure its queue, not
    // the relay. A lost reply keeps its slot; the deadline ends the run.
    while replies < args.bench {
        if sent < args.bench && sent - replies < WINDOW {
            link.send_packet(&icmp::echo_request(
                phone_ip,
                target,
                id,
                u16::try_from(sent & 0xffff).expect("masked"),
            ))
            .await?;
            sent += 1;
            if sent == args.bench / 2 {
                args.crash("during_traffic");
            }
            continue;
        }
        tokio::select! {
            incoming = link.recv() => {
                if let Incoming::Packet(packet) = incoming? {
                    replies += u32::from(icmp::is_echo_reply(packet, id));
                }
            }
            _ = &mut deadline => break,
        }
    }
    let elapsed = started.elapsed();
    println!(
        "BENCH sent={sent} replies={replies} elapsed_ms={} rtt_avg_us={}",
        elapsed.as_millis(),
        elapsed
            .as_micros()
            .checked_div(u128::from(replies))
            .unwrap_or(0)
    );
    Ok(replies)
}
