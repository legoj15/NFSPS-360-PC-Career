//! Panic containment for the background scan/convert threads: a worker
//! panic must surface as an error message, never a stranded UI.

use nfspc_converter::app::worker::{Guarded, panic_message, run_guarded};

#[test]
fn completed_work_comes_back_as_done() {
    let out = run_guarded(|| 40 + 2);
    assert!(matches!(out, Guarded::Done(42)));
}

#[test]
fn panicked_work_comes_back_with_its_message() {
    let out = run_guarded(|| panic!("boom: disk went away"));
    let Guarded::Panicked(msg) = out else {
        panic!("expected Panicked, got {out:?}");
    };
    assert!(msg.contains("boom: disk went away"), "got {msg:?}");
}

#[test]
fn panic_payload_kinds_are_extracted() {
    let p: Box<dyn std::any::Any + Send> = Box::new("str payload");
    assert_eq!(panic_message(&p), "str payload");
    let p: Box<dyn std::any::Any + Send> = Box::new(String::from("owned payload"));
    assert_eq!(panic_message(&p), "owned payload");
    let p: Box<dyn std::any::Any + Send> = Box::new(1234u32);
    assert_eq!(panic_message(&p), "unknown panic");
}
