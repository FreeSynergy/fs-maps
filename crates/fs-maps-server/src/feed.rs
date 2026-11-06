//! Changes of the provider to a subscribed consumer.
//!
//! Pattern: Observer + Adapter — the core calls its listener on the thread that made the change; this
//! one only puts the change into a channel. The shared forwarder adds the envelope and sends it.

use std::sync::Arc;

use fs_interfaces::Unsupported;
use fs_interfaces::common_capnp::{change_listener, subscription};
use fs_interfaces::rpc::{ChangeSender, Notice, change_channel, forward, unsupported};
use fs_maps_core::{PlaceChange, PlaceFeed, PlaceListener, Places};

struct ToChannel(ChangeSender<PlaceChange>);

impl PlaceListener for ToChannel {
    fn changed(&self, change: &PlaceChange) {
        self.0.send(*change);
    }
}

const SUBSCRIBE: &str = "place.subscribe";

/// A provider without a feed says so in its report; the capability would otherwise stay silent.
pub(crate) fn offered(places: &dyn Places) -> capnp::Result<()> {
    let report = places.capabilities();
    if report.is_supported(SUBSCRIBE) {
        return Ok(());
    }
    let why = report
        .why_not(SUBSCRIBE)
        .unwrap_or_else(|| Unsupported::new(SUBSCRIBE, "this provider does not offer it"));
    Err(unsupported(&why))
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

/// A place that is gone again by now has nothing to report but its own `Deleted`, which follows. A
/// read that fails is no such thing: the change cannot be told, and that ends the feed.
async fn notice(places: &dyn Places, change: PlaceChange) -> Result<Option<Notice>, String> {
    let current = |id| async move { places.get(id).await.map_err(|e| e.to_string()) };
    Ok(match change {
        PlaceChange::Created(id) => current(id)
            .await?
            .map(|place| Notice::Created(place.envelope)),
        PlaceChange::Updated(id) => current(id)
            .await?
            .map(|place| Notice::Updated(place.envelope)),
        PlaceChange::Deleted(id) => Some(Notice::Deleted(id)),
    })
}
