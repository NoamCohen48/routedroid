//! The daemon's error: what went wrong, classified by the wire's [`Kind`]
//! where it happened, so the answer to the client needs no guessing.

use std::fmt;

pub use routedroid_ipc::Kind;

#[derive(Debug)]
pub struct Fault {
    kind: Kind,
    source: anyhow::Error,
}

impl Fault {
    pub fn new(kind: Kind, source: impl Into<anyhow::Error>) -> Self {
        Self {
            kind,
            source: source.into(),
        }
    }

    pub fn msg(kind: Kind, message: impl fmt::Display) -> Self {
        Self {
            kind,
            source: anyhow::anyhow!("{message}"),
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#}", self.source)
    }
}

impl std::error::Error for Fault {}

pub type Result<T> = std::result::Result<T, Fault>;

/// `.fault(Kind::Adb)` on any `anyhow`/`std` result.
pub trait FaultExt<T> {
    fn fault(self, kind: Kind) -> Result<T>;
}

impl<T, E: Into<anyhow::Error>> FaultExt<T> for std::result::Result<T, E> {
    fn fault(self, kind: Kind) -> Result<T> {
        self.map_err(|e| Fault::new(kind, e))
    }
}
