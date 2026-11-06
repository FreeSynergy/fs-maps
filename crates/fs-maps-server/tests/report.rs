//! The provider rule over the wire (ADR 0045): reports, and "not possible" as a typed problem.

mod common;

use std::rc::Rc;
use std::sync::Arc;

use async_trait::async_trait;
use capnp_rpc::new_client;
use common::{point, serving, store_places, with_server};
use fs_interfaces::common_capnp::change_listener;
use fs_interfaces::rpc::{Problem, is_unimplemented, write_object_id};
use fs_interfaces::{Capabilities, UNSUPPORTED_PROBLEM_TYPE};
use fs_maps_core::{NoSource, Paging, Places, Result};
use fs_store::{ObjectId, StoredObject};

#[tokio::test(flavor = "current_thread")]
async fn the_report_names_all_the_store_backed_provider_offers() {
    with_server(|maps| async move {
        let answer = maps
            .places
            .support_report_request()
            .send()
            .promise
            .await
            .unwrap();
        let report = answer.get().unwrap().get_report().unwrap();
        assert_eq!(report.get_provider().unwrap().to_str().unwrap(), "fs-maps");
        let version = report.get_contract_version().unwrap().to_str().unwrap();
        assert_eq!(version, "places 0.1 (draft)");
        let caps = Capabilities::read(report).unwrap();
        for id in [
            "place.create",
            "place.update",
            "place.delete",
            "place.read",
            "place.find",
            "place.subscribe",
        ] {
            assert!(caps.is_supported(id), "{id}");
        }
        assert_eq!(caps.entries().len(), 6);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_interface_the_program_does_not_serve_is_unimplemented() {
    with_server(|maps| async move {
        let other = fs_interfaces::tasks_capnp::tasks::Client {
            client: maps.places.client.clone(),
        };
        let e = other.boards_request().send().promise.await.err().unwrap();
        assert!(is_unimplemented(&e));
    })
    .await;
}

/// A real provider of another kind: a list it cannot change, and no live feed.
struct Fixed;

#[async_trait]
impl Places for Fixed {
    fn capabilities(&self) -> Capabilities {
        let mut c = Capabilities::new("fixed");
        c.supported("place.read");
        c.unsupported("place.subscribe", "this list never changes");
        c
    }
    async fn get(&self, _: ObjectId) -> Result<Option<StoredObject>> {
        Ok(None)
    }
    async fn list(&self, _: Paging) -> Result<Vec<StoredObject>> {
        Ok(vec![])
    }
}

struct Deaf;
impl change_listener::Server for Deaf {
    async fn change(
        self: Rc<Self>,
        _: change_listener::ChangeParams,
    ) -> std::result::Result<(), capnp::Error> {
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn what_a_provider_does_not_offer_answers_not_possible() {
    let feed = store_places().await;
    serving(
        Arc::new(Fixed),
        feed,
        Arc::new(NoSource),
        |maps| async move {
            let p = maps.places;
            let check = |e: capnp::Error, capability: &str| {
                assert!(is_unimplemented(&e));
                let problem = Problem::from_error(&e).unwrap();
                assert_eq!(problem.problem_type, UNSUPPORTED_PROBLEM_TYPE);
                assert_eq!(problem.status, 501);
                assert!(problem.detail.starts_with(capability), "{}", problem.detail);
            };
            let mut create = p.create_request();
            create.get().set_geojson(point("x", 0.0, 0.0).as_str());
            check(create.send().promise.await.err().unwrap(), "place.create");

            let mut update = p.update_request();
            write_object_id(update.get().init_id(), ObjectId::new());
            update.get().set_geojson(point("x", 0.0, 0.0).as_str());
            check(update.send().promise.await.err().unwrap(), "place.update");

            let mut delete = p.delete_request();
            write_object_id(delete.get().init_id(), ObjectId::new());
            check(delete.send().promise.await.err().unwrap(), "place.delete");

            let mut within = p.within_request();
            within.get().init_box();
            check(within.send().promise.await.err().unwrap(), "place.find");

            let mut named = p.find_by_name_request();
            named.get().set_term("x");
            check(named.send().promise.await.err().unwrap(), "place.find");

            let mut subscribe = p.subscribe_request();
            subscribe
                .get()
                .set_listener(new_client::<change_listener::Client, _>(Deaf));
            let e = subscribe.send().promise.await.err().unwrap();
            check(e, "place.subscribe");
        },
    )
    .await;
}
