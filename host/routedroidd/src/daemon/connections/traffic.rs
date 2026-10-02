//! Counters for the active connections, at most once a second and only when
//! they moved. The table publishes them because they are its own business;
//! nothing else has to poll it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use routedroid_ipc::{ConnectionState, Event, Traffic};

use super::{lock, Table};
use crate::daemon::events::EventBus;

const TICK: Duration = Duration::from_secs(1);

pub(super) async fn ticker(live: Arc<Mutex<Table>>, events: EventBus) {
    let mut last = HashMap::new();
    loop {
        tokio::time::sleep(TICK).await;
        let now = active(&lock(&live));
        for (serial, traffic) in changed(&last, &now) {
            events.publish(Event::Traffic { serial, traffic });
        }
        last = now;
    }
}

fn active(table: &Table) -> HashMap<String, Traffic> {
    table
        .iter()
        .filter(|(_, connection)| connection.state() == ConnectionState::Active)
        .map(|(serial, connection)| (serial.clone(), connection.traffic()))
        .collect()
}

/// The connections whose counters differ from the last tick's, including
/// one that just became active.
fn changed(
    last: &HashMap<String, Traffic>,
    now: &HashMap<String, Traffic>,
) -> Vec<(String, Traffic)> {
    let mut moved: Vec<_> = now
        .iter()
        .filter(|(serial, traffic)| last.get(*serial) != Some(traffic))
        .map(|(serial, traffic)| (serial.clone(), *traffic))
        .collect();
    moved.sort_by(|a, b| a.0.cmp(&b.0));
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_moved_counters_are_published() {
        let quiet = Traffic::default();
        let busy = Traffic {
            packets_to_phone: 3,
            ..quiet
        };
        let last = HashMap::from([("a".to_string(), quiet), ("b".to_string(), quiet)]);
        let now = HashMap::from([
            ("a".to_string(), quiet),
            ("b".to_string(), busy),
            ("c".to_string(), quiet),
        ]);
        let moved = changed(&last, &now);
        assert_eq!(
            moved,
            vec![("b".to_string(), busy), ("c".to_string(), quiet)]
        );
        assert!(changed(&now, &now).is_empty());
    }
}
