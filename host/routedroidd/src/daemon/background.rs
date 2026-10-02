//! A background loop tied to the component that owns it: dropping the owner
//! stops the loop, so no one has to remember to shut it down.

use tokio::task::JoinHandle;

pub struct Background(JoinHandle<()>);

impl Background {
    pub fn spawn(loop_body: impl Future<Output = ()> + Send + 'static) -> Self {
        Self(tokio::spawn(loop_body))
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        self.0.abort();
    }
}
