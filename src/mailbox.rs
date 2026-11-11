//! A bounded latest-result inbox. Window messages are wakeups, never owners.
use std::sync::Mutex;

pub struct Mailbox<T>(Mutex<Slot<T>>);
struct Slot<T> {
    value: Option<T>,
    closed: bool,
}

impl<T> Default for Mailbox<T> {
    fn default() -> Self {
        Self(Mutex::new(Slot {
            value: None,
            closed: false,
        }))
    }
}

impl<T> Mailbox<T> {
    pub fn publish(&self, value: T) -> bool {
        let replaced = {
            let mut slot = self.0.lock().unwrap_or_else(|error| error.into_inner());
            if slot.closed {
                return false;
            }
            slot.value.replace(value)
        };
        // Dropping an old archive/document must not hold the inbox lock.
        drop(replaced);
        true
    }

    pub fn take(&self) -> Option<T> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .value
            .take()
    }

    pub fn close(&self) {
        let old = {
            let mut slot = self.0.lock().unwrap_or_else(|error| error.into_inner());
            slot.closed = true;
            slot.value.take()
        };
        drop(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct Counted(Arc<AtomicUsize>);
    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn replacing_and_closing_release_every_result() {
        let drops = Arc::new(AtomicUsize::new(0));
        let inbox = Mailbox::default();
        assert!(inbox.publish(Counted(drops.clone())));
        assert!(inbox.publish(Counted(drops.clone())));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        inbox.close();
        assert!(!inbox.publish(Counted(drops.clone())));
        assert!(inbox.take().is_none());
        assert_eq!(drops.load(Ordering::SeqCst), 3);
    }
}
