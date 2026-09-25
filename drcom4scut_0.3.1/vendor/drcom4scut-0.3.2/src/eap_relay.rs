//! Each UDP generation owns a separate inbox. Replay and publication share one
//! lock, so a rebuild cannot lose a SUCCESS or replay one after invalidation.
use std::sync::Mutex;

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::util::ChannelData;

#[derive(Default)]
pub struct EapRelay {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    latest: Option<ChannelData>,
    active: Option<Sender<ChannelData>>,
}

impl EapRelay {
    pub fn subscribe(&self) -> Receiver<ChannelData> {
        let (tx, rx) = unbounded();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = &inner.latest {
            // This newly created receiver is alive and the channel is unbounded.
            let _ = tx.send(state.clone());
        }
        inner.active = Some(tx); // Disconnect the old generation, never reuse it.
        rx
    }

    pub fn publish(&self, state: ChannelData) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // STOP/SLEEP/QUIT replace SUCCESS too; an invalid session is not replayed.
        inner.latest = Some(state.clone());
        if inner
            .active
            .as_ref()
            .is_some_and(|tx| tx.send(state).is_err())
        {
            inner.active = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::State;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    fn success(value: u8) -> ChannelData {
        ChannelData {
            state: State::Success,
            data: vec![value; 16],
        }
    }

    #[test]
    fn old_blocked_receiver_cannot_steal_replayed_or_live_success() {
        let relay = EapRelay::default();
        relay.publish(success(1));
        for _ in 0..64 {
            let old = relay.subscribe();
            assert_eq!(old.recv().unwrap().data, vec![1; 16]);
            let waiting = Arc::new(Barrier::new(2));
            let worker = {
                let waiting = waiting.clone();
                thread::spawn(move || {
                    waiting.wait();
                    old.recv_timeout(Duration::from_secs(2))
                })
            };
            waiting.wait();
            let new = relay.subscribe();
            relay.publish(success(2));
            assert_eq!(new.recv().unwrap().data, vec![1; 16]);
            assert_eq!(new.recv().unwrap().data, vec![2; 16]);
            assert!(matches!(
                worker.join().unwrap(),
                Err(crossbeam_channel::RecvTimeoutError::Disconnected)
            ));
            relay.publish(success(1));
        }
    }

    #[test]
    fn stop_sleep_quit_invalidate_cached_success_even_without_a_subscriber() {
        for state in [State::Stop, State::Sleep, State::Quit] {
            let relay = EapRelay::default();
            relay.publish(success(3));
            drop(relay.subscribe());
            relay.publish(ChannelData {
                state: state.clone(),
                data: Vec::new(),
            });
            let rx = relay.subscribe();
            let replay = rx.recv().unwrap();
            assert_eq!(
                std::mem::discriminant(&replay.state),
                std::mem::discriminant(&state)
            );
            assert!(replay.data.is_empty());
            relay.publish(success(4));
            assert_eq!(rx.recv().unwrap().data, vec![4; 16]);
        }
    }

    #[test]
    fn concurrent_subscribe_and_publish_never_lose_or_reorder_success() {
        for _ in 0..64 {
            let relay = Arc::new(EapRelay::default());
            relay.publish(success(1));
            let start = Arc::new(Barrier::new(2));
            let publisher = {
                let relay = relay.clone();
                let start = start.clone();
                thread::spawn(move || {
                    start.wait();
                    relay.publish(success(2));
                })
            };
            start.wait();
            let rx = relay.subscribe();
            publisher.join().unwrap();
            let messages: Vec<_> = rx.try_iter().map(|m| m.data[0]).collect();
            assert!(messages == [2] || messages == [1, 2]);
        }
    }

    #[test]
    fn invalidation_racing_rebuild_cannot_leave_a_cached_success_at_the_end() {
        for _ in 0..64 {
            let relay = Arc::new(EapRelay::default());
            relay.publish(success(1));
            let start = Arc::new(Barrier::new(2));
            let invalidator = {
                let relay = relay.clone();
                let start = start.clone();
                thread::spawn(move || {
                    start.wait();
                    relay.publish(ChannelData {
                        state: State::Stop,
                        data: Vec::new(),
                    });
                })
            };
            start.wait();
            let rx = relay.subscribe();
            invalidator.join().unwrap();
            let last = rx.try_iter().last().unwrap();
            assert!(matches!(last.state, State::Stop) && last.data.is_empty());
            assert!(matches!(
                relay.subscribe().recv().unwrap().state,
                State::Stop
            ));
        }
    }
}
