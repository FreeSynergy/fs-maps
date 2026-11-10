//! `fs-maps`: one binary for everything (ADR 0063). Pattern: Facade over the crates that stay
//! libraries.
//!
//! ```text
//! fs-maps                          the window (feature `gui`, on by default)
//! fs-maps [--socket F] [--size WxH] [--snapshot F.png]
//! fs-maps --version                the version
//! fs-maps serve                    the server (Cap'n Proto, places.capnp)
//! ```

use std::process::ExitCode;

use fs_maps_server::ServeError;

fn main() -> ExitCode {
    // The words as the system gave them: a path need not be UTF-8 (the server reads them as they are).
    let raw: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let text = |i: usize| raw.get(i).and_then(|w| w.to_str());
    match text(0) {
        Some("--version" | "-V") => {
            println!("fs-maps {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("serve") => serve(&raw[1..]),
        // `fs-maps --socket PATH serve`: the option may stand before the word.
        Some("--socket") if text(2) == Some("serve") => {
            let mut words = vec![raw[0].clone(), raw[1].clone()];
            words.extend(raw[3..].iter().cloned());
            serve(&words)
        }
        None | Some("gui") => window(&lossy(raw.get(1..).unwrap_or_default())),
        Some(_) => window(&lossy(&raw)),
    }
}

fn lossy(words: &[std::ffi::OsString]) -> Vec<String> {
    words
        .iter()
        .map(|w| w.to_string_lossy().into_owned())
        .collect()
}

fn serve(words: &[std::ffi::OsString]) -> ExitCode {
    match fs_maps_server::serve(words.iter().cloned()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(ServeError::Usage(m)) => {
            eprintln!("fs-maps: {m}");
            ExitCode::from(2)
        }
        Err(ServeError::Failed(m)) => {
            eprintln!("fs-maps: {m}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "gui")]
fn window(words: &[String]) -> ExitCode {
    match fs_maps_app::gui::run(words) {
        Ok(()) => ExitCode::SUCCESS,
        Err(m) => {
            eprintln!("fs-maps: {m}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "gui"))]
fn window(_words: &[String]) -> ExitCode {
    eprintln!(
        "fs-maps: this build has no window (built with --no-default-features); use `fs-maps serve`"
    );
    ExitCode::FAILURE
}
