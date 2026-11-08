//! `fs-maps`: one binary for everything (ADR 0063). Pattern: Facade over the crates that stay
//! libraries.
//!
//! ```text
//! fs-maps                          the window (feature `gui`, on by default)
//! fs-maps [--socket F] [--size WxH] [--snapshot F.png]
//! fs-maps serve                    the server (Cap'n Proto, places.capnp)
//! ```

use std::process::ExitCode;

use fs_maps_server::ServeError;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("serve") => serve(&args[1..]),
        // `fs-maps --socket PATH serve`: the option may stand before the word.
        Some("--socket") if args.get(2).map(String::as_str) == Some("serve") => {
            let mut words = vec!["--socket".to_string(), args[1].clone()];
            words.extend(args[3..].iter().cloned());
            serve(&words)
        }
        None | Some("gui") => window(args.get(1..).unwrap_or_default()),
        Some(_) => window(&args),
    }
}

fn serve(words: &[String]) -> ExitCode {
    match fs_maps_server::serve(words.iter().map(Into::into)) {
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
