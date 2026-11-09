//! The watch role of the supervisor (ADR 0147, step 1a): a connection that is told when a process
//! has output, and how it works with an engine or a supervisor from before the role.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_protocol::frame::{read_json, write_json};
use soyeht_ptyd::{
    Error,
    client::{Client, Notice, Watcher},
    wire::*,
    wire_kind,
};
use std::{
    os::unix::net::UnixListener,
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

mod support;
use support::*;

const QUIET: Duration = Duration::from_millis(600);
const WAIT: Duration = ASYNC_ASSERTION_TIMEOUT;

/// Reads the notices of a watcher on a thread of its own, so a test can wait for one with a timeout.
fn notices(mut watcher: Watcher) -> Receiver<Result<Notice, Error>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        loop {
            let notice = watcher.notice();
            let failed = notice.is_err();
            if sender.send(notice).is_err() || failed {
                return;
            }
        }
    });
    receiver
}

fn readable(receiver: &Receiver<Result<Notice, Error>>, instance_id: &str) -> Readable {
    loop {
        match receiver.recv_timeout(WAIT).expect("a notice").unwrap() {
            Notice::Readable(readable) if readable.instance_id == instance_id => return readable,
            _ => {}
        }
    }
}

fn watch_socket(daemon: &Daemon) -> std::path::PathBuf {
    daemon.directory.join("ptyd.sock")
}

#[test]
fn a_process_with_output_is_announced_once_until_the_engine_reads() {
    let (daemon, mut client) = Daemon::start();
    let mut watcher = Watcher::connect(&watch_socket(&daemon), 1).unwrap();
    assert!(watcher.watch(Vec::new()).unwrap().is_empty());
    let receiver = notices(watcher);
    let instance = spawn(&mut client, daemon.shell());

    input(&mut client, &instance.instance_id, "printf one\n");
    let first = readable(&receiver, &instance.instance_id);
    assert!(!first.exited);
    assert!(first.end_seq > 0);
    // More output before the engine reads is not announced again: one notice per read.
    input(&mut client, &instance.instance_id, "printf two\n");
    thread::sleep(QUIET);
    assert!(
        receiver.try_recv().is_err(),
        "a second notice before a read"
    );

    // A read clears it, and the next output is announced again.
    let mut offset = 0;
    wait_text(&mut client, &instance.instance_id, &mut offset, "two");
    input(&mut client, &instance.instance_id, "printf three\n");
    let again = readable(&receiver, &instance.instance_id);
    assert!(again.end_seq > first.end_seq);
}

#[test]
fn an_exit_is_announced_after_the_last_byte_of_output() {
    let (daemon, mut client) = Daemon::start();
    let watcher = Watcher::connect(&watch_socket(&daemon), 1).unwrap();
    let receiver = notices(watcher);
    let mut config = daemon.shell();
    config.argv = vec!["/bin/sh".into(), "-c".into(), "printf last".into()];
    let instance = spawn(&mut client, config);

    let exit = loop {
        let notice = readable(&receiver, &instance.instance_id);
        if notice.exited {
            break notice;
        }
    };
    let output = drain(&mut client, &instance.instance_id, 0);
    assert!(output.exited, "the exit is known when it is announced");
    assert_eq!(exit.end_seq, output.end_seq);
    assert_eq!(
        output.next_seq, output.end_seq,
        "the last byte comes with it"
    );
}

#[test]
fn what_was_missed_before_the_watch_is_in_its_answer() {
    let (daemon, mut client) = Daemon::start();
    let quiet = spawn(&mut client, daemon.shell());
    let busy = spawn(&mut client, daemon.shell());
    input(&mut client, &busy.instance_id, "printf there\n");
    // The output is in the supervisor before any watch exists: nobody was told.
    let mut offset = 0;
    wait_text(&mut client, &busy.instance_id, &mut offset, "there");
    let mut watcher = Watcher::connect(&watch_socket(&daemon), 1).unwrap();

    let level = watcher
        .watch(vec![
            Watched {
                instance_id: busy.instance_id.clone(),
                from_seq: 0,
            },
            Watched {
                instance_id: quiet.instance_id.clone(),
                from_seq: 0,
            },
        ])
        .unwrap();

    let ids: Vec<_> = level.iter().map(|r| r.instance_id.as_str()).collect();
    assert!(ids.contains(&busy.instance_id.as_str()), "{level:?}");
    // A process the engine has read to the end is not in the answer.
    let read_to = level
        .iter()
        .find(|r| r.instance_id == busy.instance_id)
        .unwrap()
        .end_seq;
    let level = watcher
        .watch(vec![Watched {
            instance_id: busy.instance_id.clone(),
            from_seq: read_to,
        }])
        .unwrap();
    assert!(
        level.iter().all(|r| r.instance_id != busy.instance_id),
        "{level:?}"
    );
}

#[test]
fn a_watch_does_not_take_the_place_of_the_request_connection() {
    let (daemon, mut client) = Daemon::start();
    let _watcher = Watcher::connect(&watch_socket(&daemon), 1).unwrap();
    // The request connection still works, and a second request connection is still refused.
    assert!(matches!(
        client.request(Request::List).unwrap(),
        Response::Instances(_)
    ));
    match Client::connect(&watch_socket(&daemon), 1) {
        Err(Error::Remote { kind, .. }) => assert_eq!(kind, wire_kind::ENGINE_CONNECTED),
        other => panic!("a second request connection was let in: {:?}", other.err()),
    }
}

#[test]
fn a_watch_is_bound_to_the_generation_of_the_request_connection() {
    let (daemon, client) = Daemon::start();
    for generation in [0, 2] {
        match Watcher::connect(&watch_socket(&daemon), generation) {
            Err(Error::Remote { kind, .. }) => assert!(
                kind == wire_kind::STALE_ENGINE_GENERATION || kind == wire_kind::UNAUTHORIZED,
                "{kind}"
            ),
            other => panic!("generation {generation}: {:?}", other.err()),
        }
    }
    // A watch of the right generation is closed when a newer request connection arrives.
    let receiver = notices(Watcher::connect(&watch_socket(&daemon), 1).unwrap());
    drop(client);
    let _newer = daemon.connect(2);
    while receiver.recv_timeout(WAIT).expect("the watch ends").is_ok() {}
}

#[test]
fn an_engine_from_before_the_role_is_served_as_it_always_was() {
    // The request hello of today has no role, and does not say one.
    let hello = serde_json::to_value(Request::Hello {
        version: VERSION,
        engine_generation: 1,
        role: Role::Request,
    })
    .unwrap();
    assert_eq!(
        hello,
        serde_json::json!({"method":"hello","params":{"version":VERSION,"engine_generation":1}})
    );
    let (daemon, _client) = Daemon::start();
    drop(_client);
    let mut stream = soyeht_platform::local::connect(&watch_socket(&daemon)).unwrap();
    write_json(&mut stream, &hello).unwrap();
    let welcome: serde_json::Value = read_json(&mut stream).unwrap().unwrap();
    assert_eq!(welcome["status"], "welcome");
    assert_eq!(
        welcome["value"]["features"],
        serde_json::json!([FEATURE_WATCH])
    );
}

/// A supervisor from before the role, as a stub that answers the hello a watch sends the way the
/// old server does.
fn old_supervisor(answer: serde_json::Value) -> std::path::PathBuf {
    let path = std::path::PathBuf::from(format!(
        "/tmp/soyeht-old-ptyd-{}.sock",
        uuid::Uuid::new_v4()
    ));
    let listener = UnixListener::bind(&path).unwrap();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let hello: Option<serde_json::Value> = read_json(&mut stream).unwrap();
            assert!(hello.is_some());
            write_json(&mut stream, &answer).unwrap();
        }
    });
    path
}

#[test]
fn an_engine_that_meets_a_supervisor_from_before_the_role_goes_on_polling() {
    // One that welcomes the hello as any other, with no features...
    let welcomed = old_supervisor(serde_json::json!(
        {"status":"welcome","value":{"version":VERSION,"supervisor_id":"old","supervisor_generation":1}}
    ));
    assert!(matches!(
        Watcher::connect(&welcomed, 1),
        Err(Error::WatchUnavailable)
    ));
    // ...and one that, as the old server does for a second connection of the same generation,
    // refuses it.
    let refused = old_supervisor(serde_json::json!(
        {"status":"error","value":{"kind":wire_kind::ENGINE_CONNECTED,"message":"an engine is already connected"}}
    ));
    assert!(matches!(
        Watcher::connect(&refused, 1),
        Err(Error::WatchUnavailable)
    ));
    // The request connection of a supervisor like that is read as it always was.
    let request = old_supervisor(serde_json::json!(
        {"status":"welcome","value":{"version":VERSION,"supervisor_id":"old","supervisor_generation":1}}
    ));
    let client = Client::connect(&request, 1).unwrap();
    assert!(client.features.is_empty());
}

#[test]
fn a_notice_that_comes_before_the_answer_is_part_of_it() {
    // A supervisor that writes the notice of a process's first chunk before it answers `Watch`.
    let path = std::path::PathBuf::from(format!("/tmp/soyeht-early-{}.sock", uuid::Uuid::new_v4()));
    let listener = UnixListener::bind(&path).unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _hello: Option<serde_json::Value> = read_json(&mut stream).unwrap();
        write_json(
            &mut stream,
            &serde_json::json!({"status":"welcome","value":{"version":VERSION,
                "supervisor_id":"early","supervisor_generation":1,"features":[FEATURE_WATCH]}}),
        )
        .unwrap();
        let _watch: Option<serde_json::Value> = read_json(&mut stream).unwrap();
        let early = serde_json::json!({"status":"readable","value":
            {"instance_id":"fresh","end_seq":7,"exited":false}});
        write_json(&mut stream, &early).unwrap();
        write_json(
            &mut stream,
            &serde_json::json!({"status":"watching","value":[]}),
        )
        .unwrap();
    });
    let mut watcher = Watcher::connect(&path, 1).unwrap();

    let level = watcher.watch(Vec::new()).unwrap();

    assert_eq!(
        level,
        [Readable {
            instance_id: "fresh".into(),
            end_seq: 7,
            exited: false
        }]
    );
}

#[test]
fn at_most_four_watches_are_kept_and_the_oldest_makes_room() {
    let (daemon, _client) = Daemon::start();
    let mut watches: Vec<_> = (0..5)
        .map(|_| Watcher::connect(&watch_socket(&daemon), 1).unwrap())
        .collect();
    let oldest = watches.remove(0);
    let receiver = notices(oldest);
    assert!(
        receiver
            .recv_timeout(WAIT)
            .expect("the oldest ends")
            .is_err()
    );
}
