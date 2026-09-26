//! The journal's text form and its state machine, free of I/O.
//!
//! Line 1 is the [`Header`]: format version, session, and the reservation
//! (TUN name and phone address) no other session may take while this file
//! exists. Every further line is a [`Record`] moving one step through
//! `pending → done → undo_pending → undone` (a failed apply may go straight
//! from `pending` to `undo_pending`). A final line without its `\n` is a
//! write torn by a crash and counts as never written; anything else that
//! does not parse, or breaks the state machine, fails closed.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use anyhow::{bail, ensure, Context, Result};
use routedroid_helper_ipc::IfName;
use serde::{Deserialize, Serialize};

use crate::op::Op;
use crate::session_id::SessionId;

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub version: u32,
    pub session: SessionId,
    #[serde(flatten)]
    pub reservation: Reservation,
}

/// What a session holds exclusively from journal creation to resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reservation {
    pub tun: IfName,
    pub phone_ip: Ipv4Addr,
}

impl Reservation {
    pub fn conflicts(&self, other: &Reservation) -> bool {
        self.tun == other.tun || self.phone_ip == other.phone_ip
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Intent recorded; the mutation may or may not have happened.
    Pending,
    Done,
    /// Undo intent recorded; the undo may or may not have happened.
    UndoPending,
    Undone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub seq: u32,
    pub phase: Phase,
    /// Present exactly on `pending`, which introduces the step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<Op>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub op: Op,
    pub phase: Phase,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Steps(BTreeMap<u32, Step>);

impl Steps {
    pub fn next_seq(&self) -> u32 {
        self.0.last_key_value().map_or(1, |(seq, _)| seq + 1)
    }

    /// Whether `record` is a legal next line, without applying it.
    pub fn check(&self, record: &Record) -> Result<()> {
        match (&record.op, self.0.get(&record.seq)) {
            (Some(_), _) => {
                ensure!(record.phase == Phase::Pending, "step {} introduced as {:?}", record.seq, record.phase);
                ensure!(record.seq == self.next_seq(), "step {} out of order", record.seq);
            }
            (None, None) => bail!("step {} moves to {:?} before it exists", record.seq, record.phase),
            (None, Some(step)) => {
                use Phase::*;
                let legal = matches!(
                    (step.phase, record.phase),
                    (Pending, Done) | (Pending, UndoPending) | (Done, UndoPending) | (UndoPending, Undone)
                );
                ensure!(legal, "step {} cannot move from {:?} to {:?}", record.seq, step.phase, record.phase);
            }
        }
        Ok(())
    }

    pub fn apply(&mut self, record: Record) -> Result<()> {
        self.check(&record)?;
        match record.op {
            Some(op) => {
                self.0.insert(record.seq, Step { op, phase: record.phase });
            }
            None => {
                if let Some(step) = self.0.get_mut(&record.seq) {
                    step.phase = record.phase;
                }
            }
        }
        Ok(())
    }

    /// Steps not yet undone, newest first: the order to undo them in.
    pub fn outstanding(&self) -> Vec<(u32, Step)> {
        let open = self.0.iter().rev().filter(|(_, step)| step.phase != Phase::Undone);
        open.map(|(seq, step)| (*seq, step.clone())).collect()
    }
}

#[derive(Debug)]
pub struct Parsed {
    pub header: Header,
    pub steps: Steps,
    /// Bytes up to and including the last `\n`; more than that is a torn tail.
    pub complete: usize,
}

pub fn parse(bytes: &[u8]) -> Result<Parsed> {
    let complete = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    let mut lines = bytes[..complete].split(|b| *b == b'\n').filter(|line| !line.is_empty());
    let first = lines.next().context("no header line")?;
    let header: Header = serde_json::from_slice(first).context("header")?;
    ensure!(header.version == VERSION, "journal format {} (this helper reads {VERSION})", header.version);
    let mut steps = Steps::default();
    for (n, line) in lines.enumerate() {
        let record: Record = serde_json::from_slice(line).with_context(|| format!("line {}", n + 2))?;
        steps.apply(record).with_context(|| format!("line {}", n + 2))?;
    }
    Ok(Parsed { header, steps, complete })
}
