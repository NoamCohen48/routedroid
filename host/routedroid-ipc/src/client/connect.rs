//! Opening a connection and agreeing on the API, with an error that says
//! which of the two failed: the advice for each is different.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use tokio::net::UnixStream;
use tokio::sync::Mutex;

use super::{Calls, Client, Events, Shared};
use crate::api::{Request, Response};
use crate::API_VERSION;

#[derive(Debug)]
pub enum ConnectError {
    /// Nothing is listening at the socket (or it may not be opened).
    Unreachable {
        path: PathBuf,
        source: std::io::Error,
    },
    /// A daemon answered, but speaks another API.
    Incompatible { daemon: String, api: u32 },
    /// A daemon answered, but the version exchange failed.
    Handshake(anyhow::Error),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable { path, source } => write!(
                f,
                "cannot reach routedroidd at {}: {source}",
                path.display()
            ),
            Self::Incompatible { daemon, api } => {
                write!(
                    f,
                    "routedroidd {daemon} speaks API {api}; this client speaks API {API_VERSION}"
                )
            }
            Self::Handshake(error) => {
                write!(f, "routedroidd did not answer the version check: {error:#}")
            }
        }
    }
}

impl std::error::Error for ConnectError {}

impl Client {
    pub async fn connect(path: &Path) -> Result<Self, ConnectError> {
        let stream =
            UnixStream::connect(path)
                .await
                .map_err(|source| ConnectError::Unreachable {
                    path: path.to_path_buf(),
                    source,
                })?;
        let client = Self::over(stream);
        match client
            .call(Request::Version)
            .await
            .map_err(ConnectError::Handshake)?
        {
            Response::Version { api, .. } if api == API_VERSION => Ok(client),
            Response::Version { daemon, api } => Err(ConnectError::Incompatible { daemon, api }),
            other => Err(ConnectError::Handshake(anyhow::anyhow!(
                "unexpected answer to version: {other:?}"
            ))),
        }
    }

    /// A client over an already connected stream, without the version check.
    pub fn over(stream: UnixStream) -> Self {
        let (read, write) = stream.into_split();
        let (shared, reader, queue) = Shared::start(read);
        let calls = Calls {
            writer: Arc::new(Mutex::new(write)),
            next_id: Arc::new(AtomicU64::new(1)),
            shared: Arc::clone(&shared),
            _reader: Arc::clone(&reader),
        };
        Self {
            calls,
            events: Events {
                queue,
                shared,
                _reader: reader,
            },
        }
    }
}
