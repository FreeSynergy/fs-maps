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

use fs_interfaces::places_capnp::{place, place_page, places, tile_source};
use fs_interfaces::rpc::{Problem, connect, read_envelope, write_uuid};
use fs_maps_core::describe::read_catalog;
use fs_types::TypeDescription;
use uuid::Uuid;

/// A place: its id, its name and its `GeoJSON` feature as the server keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// The id.
    pub id: String,
    /// The name.
    pub name: String,
    /// The `GeoJSON` `Feature`.
    pub geojson: String,
}

/// Why a call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiError {
    /// There is no such place.
    NotFound(String),
    /// The server cannot do this (the provider rule).
    Unsupported(String),
    /// The server refused or failed; the text is its own.
    Failed(String),
}

/// What the window asks of the maps program.
pub trait PlacesApi: Send + Sync {
    /// All places, in the server's order.
    ///
    /// # Errors
    /// [`ApiError`].
    fn list(&self) -> Result<Vec<Place>, ApiError>;
    /// One place, `None` if there is none.
    ///
    /// # Errors
    /// [`ApiError`].
    fn get(&self, id: &str) -> Result<Option<Place>, ApiError>;
    /// A new place from a `GeoJSON` feature.
    ///
    /// # Errors
    /// [`ApiError`].
    fn create(&self, geojson: &str) -> Result<Place, ApiError>;
    /// Replace a place with a `GeoJSON` feature.
    ///
    /// # Errors
    /// [`ApiError`].
    fn update(&self, id: &str, geojson: &str) -> Result<Place, ApiError>;
    /// Delete a place.
    ///
    /// # Errors
    /// [`ApiError`].
    fn delete(&self, id: &str) -> Result<(), ApiError>;
    /// Places whose name contains `term`, in the server's order.
    ///
    /// # Errors
    /// [`ApiError`].
    fn find_by_name(&self, term: &str) -> Result<Vec<Place>, ApiError>;
    /// Places whose bounding box meets the box `(west, south, east, north)` (`west > east` crosses 180).
    ///
    /// # Errors
    /// [`ApiError`].
    fn within(&self, area: (f64, f64, f64, f64)) -> Result<Vec<Place>, ApiError>;
    /// The data types the program describes (`Places.types`).
    ///
    /// # Errors
    /// [`ApiError`].
    fn types(&self) -> Result<Vec<TypeDescription>, ApiError>;
    /// One vector tile (uncompressed MVT); `None` = there is none.
    ///
    /// # Errors
    /// [`ApiError`].
    fn tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, ApiError>;
}

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

/// All pages of a paged answer: `page` asks for the page at a cursor.
async fn collect<F, Fut>(mut page: F) -> capnp::Result<Vec<Place>>
where
    F: FnMut(&[u8]) -> Fut,
    Fut: Future<Output = capnp::Result<(Vec<Place>, Vec<u8>)>>,
{
    let (mut found, mut cursor) = (Vec::new(), Vec::<u8>::new());
    loop {
        let (items, next) = page(&cursor).await?;
        found.extend(items);
        if next.is_empty() {
            return Ok(found);
        }
        cursor = next;
    }
}

fn page_of(page: place_page::Reader<'_>) -> capnp::Result<(Vec<Place>, Vec<u8>)> {
    let items = page
        .get_items()?
        .iter()
        .map(read_place)
        .collect::<capnp::Result<_>>()?;
    Ok((items, page.get_next()?.get_opaque()?.to_vec()))
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

fn problem(error: &capnp::Error) -> ApiError {
    match Problem::from_error(error) {
        Some(p) if p.is_unsupported() => ApiError::Unsupported(p.detail),
        Some(p) if p.status == 404 => ApiError::NotFound(p.detail),
        Some(p) => ApiError::Failed(p.detail),
        None => ApiError::Failed(error.to_string()),
    }
}

fn parse_id(id: &str) -> Result<Uuid, ApiError> {
    id.parse()
        .map_err(|_| ApiError::NotFound(format!("\"{id}\" names no place.")))
}

fn read_place(p: place::Reader<'_>) -> capnp::Result<Place> {
    let envelope = read_envelope(p.get_envelope()?)?;
    Ok(Place {
        id: envelope.id.to_string(),
        name: envelope.name,
        geojson: p.get_geojson()?.to_string()?,
    })
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
                .and_then(|a| Ok(a.get()?.get_catalog()?))
                .map_err(|e| problem(&e))?;
            let answer = catalog
                .describe_request()
                .send()
                .promise
                .await
                .map_err(|e| problem(&e))?;
            answer
                .get()
                .and_then(|r| r.get_types())
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
                .and_then(|a| Ok(a.get()?.get_source()?))
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
