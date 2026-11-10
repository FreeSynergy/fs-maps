//! `PlacesRemote`: the maps server behind a socket, as a [`PlacesApi`].
//!
//! Pattern: Proxy — a Cap'n Proto client lives on one thread, so a worker thread owns the connection
//! and every call is a job handed to it; the caller waits on a plain channel. [`PlacesApi`] is the
//! trait first: the adapter knows only it.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;

use fs_interfaces::places_capnp::{places, tile_source};
use fs_interfaces::rpc::{connect, write_uuid};
use fs_interfaces::types_capnp::type_catalog;
use fs_maps_core::describe::read_catalog;
use fs_types::TypeDescription;

pub use crate::api::{ApiError, Place, PlacesApi};
use crate::wire::{collect, page_of, parse_id, problem, read_place};

type Job = Box<dyn FnOnce(Rc<places::Client>) -> Pin<Box<dyn Future<Output = ()>>> + Send>;

/// The maps server on a socket.
pub struct PlacesRemote {
    jobs: tokio::sync::mpsc::UnboundedSender<Job>,
}

impl PlacesRemote {
    /// Connect to the server on `socket`.
    ///
    /// # Errors
    /// The text for the person when nothing answers.
    pub fn connect(socket: &Path) -> Result<Self, String> {
        let (jobs, inbox) = tokio::sync::mpsc::unbounded_channel();
        let (ready, started) = mpsc::channel();
        let socket = socket.to_path_buf();
        thread::Builder::new()
            .name("maps-rpc".into())
            .spawn(move || worker(&socket, inbox, &ready))
            .map_err(|e| e.to_string())?;
        started.recv().map_err(|e| e.to_string())??;
        Ok(Self { jobs })
    }

    fn run<T, F, Fut>(&self, make: F) -> Result<T, ApiError>
    where
        T: Send + 'static,
        F: FnOnce(Rc<places::Client>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, ApiError>> + 'static,
    {
        let (reply, answer) = mpsc::channel();
        let job: Job = Box::new(move |client| {
            Box::pin(async move {
                let _ = reply.send(make(client).await);
            })
        });
        self.jobs.send(job).map_err(|_| gone())?;
        answer.recv().map_err(|_| gone())?
    }
}

fn gone() -> ApiError {
    ApiError::Failed("the maps server went away".into())
}

fn worker(
    socket: &Path,
    mut inbox: tokio::sync::mpsc::UnboundedReceiver<Job>,
    ready: &mpsc::Sender<Result<(), String>>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            let _ = ready.send(Err(e.to_string()));
            return;
        }
    };
    tokio::task::LocalSet::new().block_on(&runtime, async move {
        let connection = match connect::<places::Client>(socket).await {
            Ok(c) => c,
            Err(e) => {
                let _ = ready.send(Err(format!(
                    "no maps server on {}: {e}; start it with `fs-maps serve`",
                    socket.display()
                )));
                return;
            }
        };
        let client = Rc::new(connection.client.clone());
        // One round trip, so a server that accepted the connection and then failed to start is found
        // here and not at the first call.
        if let Err(e) = client.support_report_request().send().promise.await {
            let _ = ready.send(Err(format!(
                "the maps server on {} does not answer: {e}",
                socket.display()
            )));
            return;
        }
        let _ = ready.send(Ok(()));
        while let Some(job) = inbox.recv().await {
            tokio::task::spawn_local(job(Rc::clone(&client)));
        }
    });
}

impl PlacesApi for PlacesRemote {
    fn list(&self) -> Result<Vec<Place>, ApiError> {
        self.run(|c| async move {
            collect(|cursor| {
                let mut request = c.list_request();
                request.get().init_cursor().set_opaque(cursor);
                async move {
                    let answer = request.send().promise.await?;
                    page_of(answer.get()?.get_page()?)
                }
            })
            .await
            .map_err(|e| problem(&e))
        })
    }

    fn find_by_name(&self, term: &str) -> Result<Vec<Place>, ApiError> {
        let term = term.to_string();
        self.run(move |c| async move {
            collect(|cursor| {
                let mut request = c.find_by_name_request();
                request.get().set_term(term.as_str());
                request.get().init_cursor().set_opaque(cursor);
                async move {
                    let answer = request.send().promise.await?;
                    page_of(answer.get()?.get_page()?)
                }
            })
            .await
            .map_err(|e| problem(&e))
        })
    }

    fn within(
        &self,
        (west, south, east, north): (f64, f64, f64, f64),
    ) -> Result<Vec<Place>, ApiError> {
        self.run(move |c| async move {
            collect(|cursor| {
                let mut request = c.within_request();
                {
                    let mut b = request.get().init_box();
                    b.set_west(west);
                    b.set_south(south);
                    b.set_east(east);
                    b.set_north(north);
                }
                request.get().init_cursor().set_opaque(cursor);
                async move {
                    let answer = request.send().promise.await?;
                    page_of(answer.get()?.get_page()?)
                }
            })
            .await
            .map_err(|e| problem(&e))
        })
    }

    fn types(&self) -> Result<Vec<TypeDescription>, ApiError> {
        self.run(|c| async move {
            let catalog = c
                .types_request()
                .send()
                .promise
                .await
                .and_then(|a| a.get()?.get_catalog())
                .map_err(|e| problem(&e))?;
            let answer = catalog
                .describe_request()
                .send()
                .promise
                .await
                .map_err(|e| problem(&e))?;
            answer
                .get()
                .and_then(type_catalog::describe_results::Reader::get_types)
                .and_then(read_catalog)
                .map_err(|e| problem(&e))
        })
    }

    fn tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, ApiError> {
        self.run(move |c| async move {
            let source: tile_source::Client = c
                .tiles_request()
                .send()
                .promise
                .await
                .and_then(|a| a.get()?.get_source())
                .map_err(|e| problem(&e))?;
            let mut request = source.tile_request();
            request.get().set_z(z);
            request.get().set_x(x);
            request.get().set_y(y);
            let answer = request.send().promise.await.map_err(|e| problem(&e))?;
            let r = answer.get().map_err(|e| problem(&e))?;
            if !r.get_found() {
                return Ok(None);
            }
            Ok(Some(r.get_mvt().map_err(|e| problem(&e))?.to_vec()))
        })
    }

    fn get(&self, id: &str) -> Result<Option<Place>, ApiError> {
        let id = parse_id(id)?;
        self.run(move |c| async move {
            let mut request = c.get_request();
            write_uuid(request.get().init_id(), Some(id));
            let answer = request.send().promise.await.map_err(|e| problem(&e))?;
            let r = answer.get().map_err(|e| problem(&e))?;
            if !r.get_found() {
                return Ok(None);
            }
            r.get_place()
                .and_then(read_place)
                .map(Some)
                .map_err(|e| problem(&e))
        })
    }

    fn create(&self, geojson: &str) -> Result<Place, ApiError> {
        let geojson = geojson.to_string();
        self.run(move |c| async move {
            let mut request = c.create_request();
            request.get().set_geojson(geojson.as_str());
            let answer = request.send().promise.await.map_err(|e| problem(&e))?;
            answer
                .get()
                .and_then(places::create_results::Reader::get_place)
                .and_then(read_place)
                .map_err(|e| problem(&e))
        })
    }

    fn update(&self, id: &str, geojson: &str) -> Result<Place, ApiError> {
        let (id, geojson) = (parse_id(id)?, geojson.to_string());
        self.run(move |c| async move {
            let mut request = c.update_request();
            write_uuid(request.get().init_id(), Some(id));
            request.get().set_geojson(geojson.as_str());
            let answer = request.send().promise.await.map_err(|e| problem(&e))?;
            answer
                .get()
                .and_then(places::update_results::Reader::get_place)
                .and_then(read_place)
                .map_err(|e| problem(&e))
        })
    }

    fn delete(&self, id: &str) -> Result<(), ApiError> {
        let id = parse_id(id)?;
        self.run(move |c| async move {
            let mut request = c.delete_request();
            write_uuid(request.get().init_id(), Some(id));
            request
                .send()
                .promise
                .await
                .map(|_| ())
                .map_err(|e| problem(&e))
        })
    }
}
