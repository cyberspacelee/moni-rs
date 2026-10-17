use std::{collections::BTreeMap, num::NonZeroUsize};

use moni_protocol::MinuteRollup;

pub struct PendingQueue {
    capacity: NonZeroUsize,
    entries: BTreeMap<i64, (MinuteRollup, bool)>,
}

impl PendingQueue {
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, rollup: MinuteRollup) -> Option<i64> {
        self.entries.insert(rollup.minute_ms, (rollup, false));
        if self.entries.len() > self.capacity.get() {
            self.entries.pop_first().map(|(minute_ms, _)| minute_ms)
        } else {
            None
        }
    }

    pub fn ack(&mut self, minute_ms: i64) {
        self.entries.remove(&minute_ms);
    }

    pub fn resend_all(&mut self) {
        for (_, sent) in self.entries.values_mut() {
            *sent = false;
        }
    }

    pub fn take_unsent(&mut self) -> Vec<MinuteRollup> {
        self.entries
            .values_mut()
            .filter_map(|(rollup, sent)| {
                if *sent {
                    return None;
                }
                *sent = true;
                Some(rollup.clone())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minute(minute_ms: i64, samples: u32) -> MinuteRollup {
        MinuteRollup {
            minute_ms,
            samples,
            ..MinuteRollup::default()
        }
    }

    #[test]
    fn bounded_queue_drops_oldest_and_sends_in_time_order() {
        let mut queue = PendingQueue::new(NonZeroUsize::new(2).unwrap());
        assert_eq!(queue.insert(minute(120_000, 1)), None);
        assert_eq!(queue.insert(minute(0, 1)), None);
        assert_eq!(queue.insert(minute(60_000, 1)), Some(0));
        let minutes: Vec<_> = queue.take_unsent().iter().map(|r| r.minute_ms).collect();
        assert_eq!(minutes, [60_000, 120_000]);
        assert!(queue.take_unsent().is_empty());
        assert_eq!(queue.insert(minute(-60_000, 1)), Some(-60_000));
    }

    #[test]
    fn ack_removes_only_matching_minute_and_reconnect_resends_the_rest() {
        let mut queue = PendingQueue::new(NonZeroUsize::new(3).unwrap());
        for ts in [0, 60_000, 120_000] {
            queue.insert(minute(ts, 1));
        }
        assert_eq!(queue.take_unsent().len(), 3);
        queue.ack(60_000);
        queue.ack(999);
        queue.resend_all();
        let minutes: Vec<_> = queue.take_unsent().iter().map(|r| r.minute_ms).collect();
        assert_eq!(minutes, [0, 120_000]);
    }

    #[test]
    fn duplicate_minute_replaces_value_and_becomes_unsent() {
        let mut queue = PendingQueue::new(NonZeroUsize::new(1).unwrap());
        queue.insert(minute(0, 1));
        queue.take_unsent();
        assert_eq!(queue.insert(minute(0, 60)), None);
        assert_eq!(queue.take_unsent(), [minute(0, 60)]);
    }
}
