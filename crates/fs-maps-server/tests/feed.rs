//! `subscribe` over a real socket.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use capnp_rpc::new_client;
use common::{create, delete, point, with_server};
use fs_interfaces::common_capnp::{change, change_listener};
use fs_interfaces::places_capnp::places;
use fs_interfaces::rpc::{read_envelope, write_object_id};

#[derive(Clone, Default)]
struct Seen(Rc<RefCell<Vec<String>>>);

impl change_listener::Server for Seen {
    async fn change(
        self: Rc<Self>,
        params: change_listener::ChangeParams,
    ) -> Result<(), capnp::Error> {
        let kind = match params.get()?.get_change()?.which()? {
            change::Which::Created(e) => format!("created {}", read_envelope(e?)?.name),
            change::Which::Updated(e) => format!("updated {}", read_envelope(e?)?.name),
            change::Which::Deleted(_) => "deleted".to_string(),
        };
        self.0.borrow_mut().push(kind);
        Ok(())
    }
}

async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

fn watch(places: &places::Client, seen: &Seen) -> impl Future<Output = Subscribed> {
    let listener: change_listener::Client = new_client(seen.clone());
    let mut sub = places.subscribe_request();
    sub.get().set_listener(listener);
    let sending = sub.send().promise;
    async move { sending.await.unwrap() }
}

type Subscribed = capnp::capability::Response<places::subscribe_results::Owned>;

#[tokio::test(flavor = "current_thread")]
async fn subscribers_see_changes_until_they_drop_the_capability() {
    with_server(|maps| async move {
        let p = maps.places;
        let (seen, other) = (Seen::default(), Seen::default());
        let answer = watch(&p, &seen).await;
        let subscription = answer.get().unwrap().get_subscription().unwrap();
        let kept = watch(&p, &other).await;

        let id = create(&p, "Watched", 13.4, 52.5).await;
        settle().await; // the envelope of a change is read when it goes out
        let mut update = p.update_request();
        write_object_id(update.get().init_id(), id);
        update
            .get()
            .set_geojson(point("Moved", 13.5, 52.5).as_str());
        update.send().promise.await.unwrap();
        delete(&p, id).await;
        settle().await;
        let expected = ["created Watched", "updated Moved", "deleted"];
        assert_eq!(*seen.0.borrow(), expected);
        assert_eq!(*other.0.borrow(), expected);

        drop(subscription);
        drop(answer);
        settle().await;
        create(&p, "Unwatched", 2.3, 48.8).await;
        settle().await;
        assert_eq!(seen.0.borrow().len(), 3, "still told after the drop");
        assert_eq!(other.0.borrow().len(), 4, "the other one ended too");
        drop(kept);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_refused_write_is_not_a_change() {
    with_server(|maps| async move {
        let p = maps.places;
        let seen = Seen::default();
        let _answer = watch(&p, &seen).await;
        let mut bad = p.create_request();
        bad.get().set_geojson("{ not json");
        assert!(bad.send().promise.await.is_err());
        settle().await;
        assert!(seen.0.borrow().is_empty());
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_object_gone_again_only_reports_its_deletion() {
    with_server(|maps| async move {
        let p = maps.places;
        let seen = Seen::default();
        let _answer = watch(&p, &seen).await;
        let mut ids = Vec::new();
        for _ in 0..3 {
            ids.push(create(&p, "Short lived", 1.0, 1.0).await);
        }
        for id in ids {
            delete(&p, id).await;
        }
        settle().await;
        let log = seen.0.borrow();
        assert_eq!(log.iter().filter(|s| *s == "deleted").count(), 3, "{log:?}");
        assert_eq!(log.last().map(String::as_str), Some("deleted"));
    })
    .await;
}
