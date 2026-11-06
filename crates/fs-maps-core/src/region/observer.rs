//! Progress reporting for downloads.
//!
//! Pattern: Observer — the command notifies a listener after every tile; the listener can
//! cancel by returning `false`. Any `FnMut(Progress) -> bool` is a listener.

use super::spec::Progress;

/// Listener for download progress.
pub trait ProgressObserver {
    /// Called after every tile. Return `false` to cancel the download.
    fn on_progress(&mut self, progress: Progress) -> bool;
}

/// Adapter: a closure as observer.
pub(super) struct FnObserver<'a>(pub &'a mut dyn FnMut(Progress) -> bool);

impl ProgressObserver for FnObserver<'_> {
    fn on_progress(&mut self, progress: Progress) -> bool {
        (self.0)(progress)
    }
}
