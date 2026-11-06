//! Changes of the provider to a subscribed consumer.
//!
//! Pattern: Observer + Adapter — the core calls its listener on the thread that made the change; this
//! one only puts the change into a channel. The shared forwarder adds the envelope and sends it.

use std::sync::Arc;

use fs_interfaces::common_capnp::{change_listener, subscription};
use fs_interfaces::rpc::{ChangeSender, Notice, change_channel, forward};
use fs_maps_core::{PlaceChange, PlaceFeed, PlaceListener, Places};

struct ToChannel(ChangeSender<PlaceChange>);

impl PlaceListener for ToChannel {
    fn changed(&self, change: &PlaceChange) {
        self.0.send(*change);
    }
}

/// Subscribe `listener` until the returned capability is dropped.
pub(crate) fn subscribe(
    feed: &Arc<dyn PlaceFeed>,
    places: Arc<dyn Places>,
    listener: change_listener::Client,
) -> subscription::Client {
    let (tx, rx) = change_channel();
    let core_subscription = feed.subscribe(Arc::new(ToChannel(tx)));
    forward(rx, core_subscription, listener, move |change| {
        let places = places.clone();
        async move { notice(places.as_ref(), change).await }
    })
}

/// A place that is gone again by now has nothing to report but its own `Deleted`, which follows.
async fn notice(places: &dyn Places, change: PlaceChange) -> Option<Notice> {
    match change {
        PlaceChange::Created(id) => Some(Notice::Created(places.get(id).await.ok()??.envelope)),
        PlaceChange::Updated(id) => Some(Notice::Updated(places.get(id).await.ok()??.envelope)),
        PlaceChange::Deleted(id) => Some(Notice::Deleted(id)),
    }
}
