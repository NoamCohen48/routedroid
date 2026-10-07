//! A connection's first step: the app the daemon carries goes on the phone
//! if the phone lacks it or has an older one. Nothing else has been set up
//! yet, so a stop here has nothing to undo.

use routedroid_ipc::ConnectionState;
use tokio::sync::watch;

use super::run::ConnectionRun;
use crate::fault::Result;

impl ConnectionRun {
    /// `Ok(true)` when stopped while installing.
    pub(super) async fn install_app(&self, stop_rx: &mut watch::Receiver<bool>) -> Result<bool> {
        let Some(app) = self.app else {
            return Ok(false);
        };
        let phone = self.adb.device(&self.spec.serial);
        let installing = || self.sink.set(ConnectionState::InstallingApp);
        tokio::select! {
            tried = app.ensure(&phone, installing) => {
                if tried? {
                    self.sink.set(ConnectionState::Starting);
                }
                Ok(false)
            }
            _ = stop_rx.changed() => Ok(true),
        }
    }
}
