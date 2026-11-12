//! A recorder of what terminal panes show, for research on telling when the person is typing.
//!
//! It connects to a running engine as an ordinary client, the way the Windows and Linux window
//! does, keeps each terminal pane's screen with the client's own screen model, and writes a JSON
//! Lines file per pane of how the cursor and the rows around it changed, with the whole screen
//! now and then. It records screens, never keystrokes, it sends the engine nothing but the
//! subscriptions to the panes' output, and it is not built into the window or any package
//! (docs/research/terminal-screen-recorder.md).
//!
//! `cargo run -p soyeht-client --example screen_recorder -- --help`, from `apps/desktop`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod args;
mod recorder;
mod store;

use args::{Parsed, USAGE};
use recorder::{Recorder, Stamp};
use soyeht_client::{Engine, Event};
use std::{
    process::ExitCode,
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use store::Store;

/// How often the recorder looks at the clock when the engine is quiet.
const TICK: Duration = Duration::from_millis(50);
/// Events handled before the screens are looked at, so that a flooding pane costs one record for
/// the whole batch and not one for every chunk.
const BATCH: usize = 256;

fn main() -> ExitCode {
    let options = match args::parse(std::env::args_os().skip(1), args::default_out()) {
        Ok(Parsed::Run(options)) => *options,
        Ok(Parsed::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("screen_recorder: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("screen_recorder: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: args::Options) -> Result<(), String> {
    // The folder that holds the default recordings is ours too: keep it owner-only.
    if let (Some(default), Some(parent)) = (args::default_out(), options.out.parent())
        && default == options.out
    {
        soyeht_platform::fs::ensure_private_dir(parent).map_err(|error| error.to_string())?;
    }
    let store = Store::new(options.out.clone(), options.limits)
        .map_err(|error| format!("cannot open {}: {error}", options.out.display()))?;
    let mut recorder = Recorder::new(store, options.panes);
    eprintln!(
        "screen_recorder: connecting to {}\nscreen_recorder: recordings go to {}/<date>/<pane>.jsonl\nscreen_recorder: Ctrl-C stops",
        options.socket.display(),
        options.out.display()
    );

    let (engine, inbox) = Engine::start(options.socket);
    let (events, queue) = mpsc::channel::<Event>();
    std::thread::spawn(move || {
        while let Ok(event) = inbox.recv_blocking() {
            if events.send(event).is_err() {
                break;
            }
        }
    });

    let started = Instant::now();
    let stamp = || Stamp {
        wall_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(0)),
        mono_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(0),
    };
    loop {
        let first = match queue.recv_timeout(TICK) {
            Ok(event) => Some(event),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        for event in first.into_iter().chain(queue.try_iter()).take(BATCH) {
            let actions = recorder.handle(event, stamp());
            for command in actions.subscribe {
                if let Err(error) = engine.send(command) {
                    eprintln!("screen_recorder: cannot subscribe to a pane: {error}");
                }
            }
            if actions.resync {
                engine.resync();
            }
        }
        let now = stamp();
        recorder.end_batch(now);
        recorder.tick(now);
        for note in recorder.take_notes() {
            eprintln!("screen_recorder: {note}");
        }
    }
    Ok(())
}
