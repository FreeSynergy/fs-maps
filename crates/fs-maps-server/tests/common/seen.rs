//! A consumer's listener that writes down everything it is told.

use std::cell::RefCell;
use std::rc::Rc;

use capnp_rpc::new_client;
use fs_interfaces::common_capnp::{change, change_listener};
use fs_interfaces::places_capnp::places;
use fs_interfaces::rpc::{read_envelope, read_object_id};
use fs_store::ObjectId;

/// One call on the listener.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Told {
    Created(ObjectId, String),
    Updated(ObjectId, String),
    Deleted(ObjectId),
    Ended(String, u16),
}

#[derive(Clone, Default)]
pub struct Seen(pub Rc<RefCell<Vec<Told>>>);

impl change_listener::Server for Seen {
    async fn change(
        self: Rc<Self>,
        params: change_listener::ChangeParams,
    ) -> Result<(), capnp::Error> {
        let told = match params.get()?.get_change()?.which()? {
            change::Which::Created(e) => {
                let e = read_envelope(e?)?;
                Told::Created(e.id, e.name)
            }
            change::Which::Updated(e) => {
                let e = read_envelope(e?)?;
                Told::Updated(e.id, e.name)
            }
            change::Which::Deleted(id) => Told::Deleted(read_object_id(id?)?),
        };
        self.0.borrow_mut().push(told);
        Ok(())
    }

    async fn ended(
        self: Rc<Self>,
        params: change_listener::EndedParams,
        _: change_listener::EndedResults,
    ) -> Result<(), capnp::Error> {
        let problem = params.get()?.get_problem()?;
        self.0.borrow_mut().push(Told::Ended(
            problem.get_type()?.to_string()?,
            problem.get_status(),
        ));
        Ok(())
    }
}

impl Seen {
    pub fn told(&self) -> Vec<Told> {
        self.0.borrow().clone()
    }
}

/// Subscribe; the answer holds the subscription until it is dropped.
pub async fn watch(
    places: &places::Client,
    seen: &Seen,
) -> capnp::capability::Response<places::subscribe_results::Owned> {
    let listener: change_listener::Client = new_client(seen.clone());
    let mut req = places.subscribe_request();
    req.get().set_listener(listener);
    req.send().promise.await.unwrap()
}

/// Long enough for what the server sends to arrive; the feed runs on this thread too.
pub async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}
