//! The window of `fs-maps` (ADR 0063): wiring only.
//!
//! Pattern: Facade consumer — connect to the server (starting it if none answers on the user's own
//! socket), build the workbench, open the window or draw one picture.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs_components_app::{
    AppError, AppWidget, Element, Screen, Surface, WindowOptions, launch, parse_size,
};
use fs_components_workbenches::people::{PeopleSource, PeopleWidget};
use fs_interfaces::rpc::default_socket;

use crate::words::{TITLE, labels};
use crate::{PlacesAdapter, PlacesRemote};

/// The help text.
pub const HELP: &str = "fs-maps: the maps window (without arguments it opens)

  --socket <path>    the maps server to talk to (default: $XDG_RUNTIME_DIR/freesynergy/maps.sock);
                     without it, a server is started if none answers; start it yourself with `fs-maps serve`
  --snapshot <png>   draw a picture instead of opening a window
  --size <WxH>       window or picture size (default 1200x800)";

struct MapsScreen(PeopleWidget);

impl Surface for MapsScreen {
    fn build(&self, width: f32, height: f32) -> Box<dyn Element> {
        Box::new(self.0.build(width, height))
    }

    fn styles(&self) -> String {
        self.0.styles()
    }
}

/// The shell around the workbench over `source`.
///
/// # Errors
/// [`AppError`] if the shell cannot be made.
pub fn app_widget(source: Arc<dyn PeopleSource>) -> Result<AppWidget, AppError> {
    let widget = PeopleWidget::new(source, labels());
    let screen = Screen::new("maps", TITLE, Arc::new(MapsScreen(widget)));
    AppWidget::new(TITLE, vec![screen])
}

struct Args {
    socket: Option<PathBuf>,
    snapshot: Option<PathBuf>,
    size: (u32, u32),
    help: bool,
}

fn parse(words: &[String]) -> Result<Args, String> {
    let mut out = Args {
        socket: None,
        snapshot: None,
        size: (1200, 800),
        help: false,
    };
    let mut it = words.iter();
    while let Some(word) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match word.as_str() {
            "--socket" => out.socket = Some(PathBuf::from(value("--socket")?)),
            "--snapshot" => out.snapshot = Some(PathBuf::from(value("--snapshot")?)),
            "--size" => out.size = parse_size(&value("--size")?)?,
            "-h" | "--help" => out.help = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(out)
}

/// Open the window. `words` are the words without the program name.
///
/// # Errors
/// The message for the person: bad words, no server, a picture that cannot be drawn.
pub fn run(words: &[String]) -> Result<(), String> {
    let args = parse(words)?;
    if args.help {
        println!("{HELP}");
        return Ok(());
    }
    let socket = args
        .socket
        .clone()
        .or_else(|| default_socket("maps"))
        .ok_or("no socket: give --socket or set XDG_RUNTIME_DIR")?;
    let remote = connect_or_start(&socket, args.socket.is_none())?;
    let adapter = PlacesAdapter::new(Arc::new(remote));
    let app = app_widget(Arc::new(adapter)).map_err(|e| e.to_string())?;
    let (width, height) = args.size;
    if let Some(file) = &args.snapshot {
        app.picture(width, height, file)
            .map_err(|e| e.to_string())?;
        eprintln!("wrote {}", file.display());
        return Ok(());
    }
    let mut options = WindowOptions::default();
    options.width = width;
    options.height = height;
    launch(options, app).map_err(|e| e.to_string())
}

/// Connect; if none answers on the user's own socket, start `fs-maps serve` (detached, so other
/// windows use it too) and wait for it. A socket given with `--socket` is never answered by starting
/// anything.
fn connect_or_start(socket: &Path, may_start: bool) -> Result<PlacesRemote, String> {
    let first = PlacesRemote::connect(socket);
    if first.is_ok() || !may_start {
        return first;
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    // What the server says when it cannot start goes to a file next to the socket, so the window can
    // tell the person instead of only waiting.
    let log = socket.with_extension("log");
    let log_file = std::fs::File::create(&log).ok();
    let mut server = std::process::Command::new(me);
    server
        .arg("serve")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(log_file.map_or_else(std::process::Stdio::null, std::process::Stdio::from));
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut server, 0);
    let mut child = server
        .spawn()
        .map_err(|e| format!("could not start the maps server: {e}"))?;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Ok(remote) = PlacesRemote::connect(socket) {
            let _ = std::fs::remove_file(&log);
            return Ok(remote);
        }
        if let Ok(Some(status)) = child.try_wait() {
            let said = std::fs::read_to_string(&log).unwrap_or_default();
            let _ = std::fs::remove_file(&log);
            return Err(format!(
                "the maps server stopped at once ({status}): {}",
                said.trim()
            ));
        }
    }
    // Still waiting: the child lives on (it may just be slow); reap it if it ends later.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    first
}
