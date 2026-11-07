//! The [`PlaceFeed`] contract: live changes of places.
//!
//! Pattern: Observer — a [`PlaceListener`] subscribes and is told about every committed change;
//! dropping the [`Subscription`] ends it (as in `common.capnp`). `Registry` is the subject. The
//! same shape as the feed of `fs-tasks-core`; it moves into a shared crate when a third one appears.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use fs_objects::ObjectId;

/// What happened to a place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceChange {
    /// A place was created.
    Created(ObjectId),
    /// A place was changed.
    Updated(ObjectId),
    /// A place was deleted.
    Deleted(ObjectId),
}

/// Receives changes. Called on the thread that made the change: keep it short, hand work off.
pub trait PlaceListener: Send + Sync {
    /// A place changed.
    fn changed(&self, change: &PlaceChange);
}

/// `place.subscribe`: live updates of places.
pub trait PlaceFeed: Send + Sync {
    /// Tell `listener` about every change until the returned [`Subscription`] is dropped.
    fn subscribe(&self, listener: Arc<dyn PlaceListener>) -> Subscription;
}

type Entries = Vec<(u64, Arc<dyn PlaceListener>)>;

/// The subject of the Observer: holds the listeners and notifies them.
#[derive(Default)]
pub(super) struct Registry {
    next: Mutex<u64>,
    listeners: Mutex<Entries>,
}

impl Registry {
    /// Add a listener; the subscription removes it again.
    pub(super) fn add(self: &Arc<Self>, listener: Arc<dyn PlaceListener>) -> Subscription {
        let mut next = self.next.lock().unwrap_or_else(PoisonError::into_inner);
        *next += 1;
        let id = *next;
        self.entries().push((id, listener));
        Subscription {
            id,
            registry: Arc::clone(self),
        }
    }

    /// Tell every listener.
    pub(super) fn notify(&self, change: &PlaceChange) {
        let listeners: Vec<_> = self.entries().iter().map(|(_, l)| l.clone()).collect();
        for l in listeners {
            l.changed(change);
        }
    }

    fn entries(&self) -> MutexGuard<'_, Entries> {
        self.listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Keeps a subscription alive; dropping it unsubscribes.
#[must_use = "dropping the subscription ends it"]
pub struct Subscription {
    id: u64,
    registry: Arc<Registry>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let id = self.id;
        self.registry.entries().retain(|(i, _)| *i != id);
    }
}
