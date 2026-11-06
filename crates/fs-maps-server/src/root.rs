//! The one capability of the program: both interfaces of `places.capnp` on one socket.
//!
//! The contract has two root interfaces and no way from one to the other, and a program has one
//! socket (`fs_interfaces::rpc`). A call names its interface, so the root sends it on by that id: a
//! consumer connects as `places::Client` or as `tile_source::Client`, on the same socket.
//!
//! Pattern: Facade + Adapter — [`MapsServer`] is what a program serves and depends on the traits,
//! never on `StorePlaces`; `Dispatch` is what the generated code would be for `Maps extends (Places,
//! TileSource)`, written by hand because the contract has no such interface.

use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;

use capnp::any_pointer;
use capnp::capability::{
    self, Client, DispatchCallResult, FromClientHook, FromServer, Params, Results,
};
use capnp::private::capability::ClientHook;
use capnp::traits::HasTypeId;
use capnp_rpc::new_client;
use fs_interfaces::places_capnp::{places, tile_source};
use fs_maps_core::{PlaceFeed, Places, TileSource};

use crate::places_service::PlacesImpl;
use crate::tiles_service::TilesImpl;

/// The root capability of the maps program.
pub struct MapsServer {
    places: Arc<dyn Places>,
    feed: Arc<dyn PlaceFeed>,
    tiles: Arc<dyn TileSource>,
}

impl MapsServer {
    /// Serve this provider and this source of tiles.
    pub fn new(
        places: Arc<dyn Places>,
        feed: Arc<dyn PlaceFeed>,
        tiles: Arc<dyn TileSource>,
    ) -> Self {
        Self {
            places,
            feed,
            tiles,
        }
    }

    /// The capability to hand to `fs_interfaces::rpc::serve`.
    #[must_use]
    pub fn into_client(self) -> Client {
        let places = PlacesImpl {
            places: self.places,
            feed: self.feed,
        };
        let tiles = TilesImpl { source: self.tiles };
        let both = Both {
            places: <places::Client as FromServer<PlacesImpl>>::from_server(Rc::new(places)),
            tiles: <tile_source::Client as FromServer<TilesImpl>>::from_server(Rc::new(tiles)),
        };
        new_client::<Root, Both>(both).client
    }
}

struct Both {
    places: <places::Client as FromServer<PlacesImpl>>::Dispatch,
    tiles: <tile_source::Client as FromServer<TilesImpl>>::Dispatch,
}

#[derive(Clone)]
struct Dispatch(Rc<Both>);

impl Deref for Dispatch {
    type Target = Both;
    fn deref(&self) -> &Both {
        &self.0
    }
}

impl capability::Server for Dispatch {
    fn dispatch_call(
        self,
        interface_id: u64,
        method_id: u16,
        params: Params<any_pointer::Owned>,
        results: Results<any_pointer::Owned>,
    ) -> DispatchCallResult {
        if interface_id == tile_source::Client::TYPE_ID {
            let tiles = self.0.tiles.clone();
            tiles.dispatch_call(interface_id, method_id, params, results)
        } else {
            // Answers `unimplemented` for an interface it does not know.
            let places = self.0.places.clone();
            places.dispatch_call(interface_id, method_id, params, results)
        }
    }

    fn as_ptr(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
}

struct Root {
    client: Client,
}

impl FromClientHook for Root {
    fn new(hook: Box<dyn ClientHook>) -> Self {
        Self {
            client: Client::new(hook),
        }
    }
    fn into_client_hook(self) -> Box<dyn ClientHook> {
        self.client.hook
    }
    fn as_client_hook(&self) -> &dyn ClientHook {
        &*self.client.hook
    }
}

impl FromServer<Both> for Root {
    type Dispatch = Dispatch;
    fn from_server(both: Rc<Both>) -> Dispatch {
        Dispatch(both)
    }
}
