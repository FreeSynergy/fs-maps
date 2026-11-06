//! `Places` interface: every method of the contract, each one call of the provider.
//!
//! Pattern: Adapter — wire request in, `Places` call, wire response out. What a provider cannot do
//! arrives as its explicit `Unsupported` and leaves as a typed problem.

use std::rc::Rc;
use std::sync::Arc;

use fs_interfaces::Unsupported;
use fs_interfaces::places_capnp::places;
use fs_interfaces::rpc::{read_object_id, unsupported};
use fs_maps_core::{Paging, PlaceFeed, Places};

use crate::errors::rpc_error;
use crate::feed::subscribe;
use crate::wire::{CONTRACT_VERSION, read_area, read_feature, read_term, write_page, write_place};

const SUBSCRIBE: &str = "place.subscribe";

pub(crate) struct PlacesImpl {
    pub(crate) places: Arc<dyn Places>,
    pub(crate) feed: Arc<dyn PlaceFeed>,
}

type Done = Result<(), capnp::Error>;

/// A page of the contract is cut by `Page`, which needs the whole ordered list (`DESIGN.md`).
const WHOLE_LIST: Paging = Paging {
    limit: None,
    offset: 0,
};

impl places::Server for PlacesImpl {
    async fn support_report(
        self: Rc<Self>,
        _: places::SupportReportParams,
        mut results: places::SupportReportResults,
    ) -> Done {
        let report = self.places.capabilities();
        report.write(CONTRACT_VERSION, results.get().init_report());
        Ok(())
    }

    async fn create(
        self: Rc<Self>,
        params: places::CreateParams,
        mut results: places::CreateResults,
    ) -> Done {
        let feature = read_feature(params.get()?.get_geojson()?)?;
        let place = self.places.create(&feature).await.map_err(rpc_error)?;
        write_place(results.get().init_place(), &place)
    }

    async fn update(
        self: Rc<Self>,
        params: places::UpdateParams,
        mut results: places::UpdateResults,
    ) -> Done {
        let p = params.get()?;
        let id = read_object_id(p.get_id()?)?;
        let feature = read_feature(p.get_geojson()?)?;
        let place = self.places.update(id, &feature).await.map_err(rpc_error)?;
        write_place(results.get().init_place(), &place)
    }

    async fn delete(
        self: Rc<Self>,
        params: places::DeleteParams,
        _: places::DeleteResults,
    ) -> Done {
        let id = read_object_id(params.get()?.get_id()?)?;
        self.places.delete(id).await.map_err(rpc_error)
    }

    async fn get(
        self: Rc<Self>,
        params: places::GetParams,
        mut results: places::GetResults,
    ) -> Done {
        let id = read_object_id(params.get()?.get_id()?)?;
        let found = self.places.get(id).await.map_err(rpc_error)?;
        let mut r = results.get();
        r.set_found(found.is_some());
        match found {
            Some(place) => write_place(r.init_place(), &place),
            None => Ok(()),
        }
    }

    async fn list(
        self: Rc<Self>,
        params: places::ListParams,
        mut results: places::ListResults,
    ) -> Done {
        let all = self.places.list(WHOLE_LIST).await.map_err(rpc_error)?;
        write_page(results.get().init_page(), all, params.get()?.get_cursor())
    }

    async fn within(
        self: Rc<Self>,
        params: places::WithinParams,
        mut results: places::WithinResults,
    ) -> Done {
        let p = params.get()?;
        let area = read_area(p)?;
        let all = self.places.within(&area, WHOLE_LIST).await;
        write_page(
            results.get().init_page(),
            all.map_err(rpc_error)?,
            p.get_cursor(),
        )
    }

    async fn find_by_name(
        self: Rc<Self>,
        params: places::FindByNameParams,
        mut results: places::FindByNameResults,
    ) -> Done {
        let p = params.get()?;
        let term = read_term(p.get_term()?)?;
        let all = self.places.find_by_name(term, WHOLE_LIST).await;
        write_page(
            results.get().init_page(),
            all.map_err(rpc_error)?,
            p.get_cursor(),
        )
    }

    /// A provider without a feed says so in its report; the capability would otherwise stay silent.
    async fn subscribe(
        self: Rc<Self>,
        params: places::SubscribeParams,
        mut results: places::SubscribeResults,
    ) -> Done {
        let report = self.places.capabilities();
        if !report.is_supported(SUBSCRIBE) {
            let why = report
                .why_not(SUBSCRIBE)
                .unwrap_or_else(|| Unsupported::new(SUBSCRIBE, "this provider does not offer it"));
            return Err(unsupported(&why));
        }
        let listener = params.get()?.get_listener()?;
        let subscription = subscribe(&self.feed, self.places.clone(), listener);
        results.get().set_subscription(subscription);
        Ok(())
    }
}
