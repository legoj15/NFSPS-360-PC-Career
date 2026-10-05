//! Panic containment for the background scan/convert threads.
//!
//! The UI flags progress by `scanning`/`converting` and clears them only
//! when the worker's message arrives. An uncaught panic on the worker
//! thread would strand the UI (perpetual spinner, Convert/Refresh
//! disabled). The guard turns a panic into an error string the UI shows.

use std::any::Any;

/// Outcome of [`run_guarded`].
#[derive(Debug)]
pub enum Guarded<T> {
    /// The closure completed normally.
    Done(T),
    /// The closure panicked; carries a best-effort message from the payload.
    Panicked(String),
}

/// Run `f` to completion, catching any panic.
pub fn run_guarded<T>(f: impl FnOnce() -> T) -> Guarded<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(v) => Guarded::Done(v),
        Err(p) => Guarded::Panicked(panic_message(&p)),
    }
}

/// Extract a readable message from a panic payload (`&str` / `String` /
/// anything else), mirroring what the default panic hook prints.
pub fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}
