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

use fs_interfaces::common_capnp;
use fs_interfaces::places_capnp::{place, places};
use fs_interfaces::rpc::{Problem, connect, read_envelope, write_uuid};
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
            let (mut found, mut cursor) = (Vec::new(), Vec::<u8>::new());
            loop {
                let mut request = c.list_request();
                request.get().init_cursor().set_opaque(&cursor);
                let answer = request.send().promise.await.map_err(|e| problem(&e))?;
                let page = answer
                    .get()
                    .and_then(places::list_results::Reader::get_page)
                    .map_err(|e| problem(&e))?;
                for p in page.get_items().map_err(|e| problem(&e))? {
                    found.push(read_place(p).map_err(|e| problem(&e))?);
                }
                let next = page
                    .get_next()
                    .and_then(common_capnp::cursor::Reader::get_opaque)
                    .map_err(|e| problem(&e))?;
                if next.is_empty() {
                    return Ok(found);
                }
                cursor = next.to_vec();
            }
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
