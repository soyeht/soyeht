use super::*;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// The test executable doubles as the helper program the tests run, so they behave the
/// same on every system: `HELPER_MODE` selects what the helper does (see [`helper`]).
const HELPER_MODE: &str = "SOYEHT_PROCESS_TEST_HELPER";

fn helper_command(mode: &str) -> Command {
    let mut command = Command::new(env::current_exe().unwrap());
    command
        .args(["--exact", "tests::helper", "--nocapture"])
        .env(HELPER_MODE, mode);
    command
}

/// Does nothing in a normal test run. As a helper it plays the program that a test runs.
#[test]
fn helper() {
    let Ok(mode) = env::var(HELPER_MODE) else {
        return;
    };
    let (mode, argument) = mode.split_once(':').unwrap_or((&mode, ""));
    match mode {
        "emit" => {
            print!("out");
            eprint!("err");
            std::process::exit(3);
        }
        "flood" => {
            print!("{}", "x".repeat(100_000));
        }
        // Starts a sleeping descendant, records its pid, and waits for it.
        "hang" => {
            let mut descendant = helper_command("sleep").spawn().unwrap();
            fs::write(argument, descendant.id().to_string()).unwrap();
            descendant.wait().unwrap();
        }
        "sleep" => thread::sleep(Duration::from_secs(30)),
        // Leaves a sleeping background process behind and ends at once.
        "detach" => {
            let descendant = helper_command("sleep").spawn().unwrap();
            fs::write(argument, descendant.id().to_string()).unwrap();
            // Only its pid matters: the test ends it.
            std::mem::forget(descendant);
        }
        // Writes what arrives on standard input back to standard output.
        "echo" => {
            let mut input = Vec::new();
            io::stdin().read_to_end(&mut input).unwrap();
            io::stdout().write_all(&input).unwrap();
        }
        _ => {}
    }
}

fn scratch(label: &str) -> PathBuf {
    env::temp_dir().join(format!("soyeht-process-{label}-{}", std::process::id()))
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid.try_into().unwrap(), 0) == 0 }
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    let listing = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&listing.stdout).contains(&format!("\"{pid}\""))
}

#[cfg(unix)]
fn end_process(pid: u32) {
    // SAFETY: the pid is a sleeping helper this test started and still owns.
    unsafe { libc::kill(pid.try_into().unwrap(), libc::SIGKILL) };
}

#[cfg(windows)]
fn end_process(pid: u32) {
    Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output()
        .unwrap();
}

fn read_pid(file: &Path) -> u32 {
    fs::read_to_string(file).unwrap().trim().parse().unwrap()
}

#[test]
fn a_command_returns_its_status_and_both_streams() {
    let output = run(
        helper_command("emit"),
        Limits::new(Duration::from_secs(30), 8192),
    )
    .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(!output.success());
    // The test harness prints its own lines around the helper's.
    assert!(String::from_utf8_lossy(&output.stdout).contains("out"));
    assert!(output.stderr_text().contains("err"));
}

#[test]
fn a_hung_command_is_killed_with_its_descendants_and_reported() {
    let pidfile = scratch("hung");
    let _ = fs::remove_file(&pidfile);
    let started = Instant::now();
    // Long enough for a loaded host to start the helper and its descendant.
    let error = run(
        helper_command(&format!("hang:{}", pidfile.display())),
        Limits::new(Duration::from_secs(5), 8192),
    )
    .unwrap_err();
    let elapsed = started.elapsed();
    assert!(error.timed_out(), "{error}");
    assert!(
        error.to_string().contains("timed out after 5000 ms"),
        "{error}"
    );
    // The deadline plus the pipe grace bounds the call, far below the
    // 30 s the descendant would have slept.
    assert!(elapsed < Duration::from_secs(20), "{elapsed:?}");
    let descendant = read_pid(&pidfile);
    let deadline = Instant::now() + Duration::from_secs(10);
    while alive(descendant) {
        assert!(
            Instant::now() < deadline,
            "descendant {descendant} survived"
        );
        thread::sleep(Duration::from_millis(10));
    }
    fs::remove_file(pidfile).unwrap();
}

#[test]
fn a_command_that_finishes_leaves_its_background_process_running() {
    let pidfile = scratch("detached");
    let _ = fs::remove_file(&pidfile);
    let mut running = starting("helper", || {
        Running::spawn(
            helper_command(&format!("detach:{}", pidfile.display())),
            false,
        )
    })
    .unwrap()
    .unwrap();
    // The pipes stay open so the helper never fails writing to them.
    let _streams = running.take_streams();
    let deadline = Instant::now() + Duration::from_secs(30);
    while running.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the helper did not finish");
        thread::sleep(Duration::from_millis(10));
    }
    running.release();
    drop(running);
    // Dropping the last handle would end the job's processes if it were not released.
    thread::sleep(Duration::from_millis(500));
    let descendant = read_pid(&pidfile);
    let survived = alive(descendant);
    end_process(descendant);
    fs::remove_file(pidfile).unwrap();
    assert!(
        survived,
        "the background process was ended with the command"
    );
}

#[test]
fn output_over_budget_is_drained_and_reported() {
    let error = run(
        helper_command("flood"),
        Limits::new(Duration::from_secs(30), 1024),
    )
    .unwrap_err();
    assert!(matches!(error, Error::OutputTooLarge { .. }), "{error}");
}

#[test]
fn input_reaches_the_command_and_a_command_that_ignores_it_is_not_stalled() {
    let echoed = run_with_input(
        helper_command("echo"),
        b"config".to_vec(),
        Limits::new(Duration::from_secs(30), 8192),
    )
    .unwrap();
    assert!(String::from_utf8_lossy(&echoed.stdout).contains("config"));
    // Larger than a pipe buffer, and never read: the writer must not hold the call.
    let started = Instant::now();
    let ignored = run_with_input(
        helper_command("quiet"),
        vec![b'x'; 1 << 20],
        Limits::new(Duration::from_secs(30), 8192),
    )
    .unwrap();
    assert!(ignored.success());
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
fn a_command_that_never_reads_its_input_times_out_instead_of_hanging() {
    let error = run_with_input(
        helper_command("sleep"),
        vec![b'x'; 1 << 20],
        Limits::new(Duration::from_secs(2), 8192),
    )
    .unwrap_err();
    assert!(error.timed_out(), "{error}");
}

#[test]
fn a_thread_holding_the_core_lock_cannot_start_a_command() {
    {
        let _held = forbid("Core");
        assert_eq!(forbidden(), Some("Core"));
        let error = run(
            helper_command("quiet"),
            Limits::new(Duration::from_secs(30), 8192),
        )
        .unwrap_err();
        assert!(
            matches!(error, Error::UnderLock { holder: "Core", .. }),
            "{error}"
        );
    }
    assert_eq!(forbidden(), None);
    assert!(
        run(
            helper_command("quiet"),
            Limits::new(Duration::from_secs(30), 8192)
        )
        .unwrap()
        .success()
    );
}

#[test]
fn a_thread_holding_the_core_lock_cannot_queue_a_start() {
    let _held = forbid("Core");
    let error = starting("helper", || ()).unwrap_err();
    assert!(
        matches!(error, Error::UnderLock { holder: "Core", .. }),
        "{error}"
    );
}

#[test]
fn commands_started_together_all_finish_and_none_reports_a_timeout() {
    let runners: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(|| {
                (0..3)
                    .map(|_| {
                        run(
                            helper_command("emit"),
                            Limits::new(Duration::from_secs(60), 8192),
                        )
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    for runner in runners {
        for result in runner.join().unwrap() {
            let output = result.unwrap();
            assert_eq!(output.status.code(), Some(3));
            assert!(String::from_utf8_lossy(&output.stdout).contains("out"));
            assert!(output.stderr_text().contains("err"));
        }
    }
}

#[test]
fn the_time_a_command_queues_behind_other_starts_counts_against_its_timeout() {
    let (held, holding) = mpsc::channel();
    let blocker = thread::spawn(move || {
        starting("helper", || {
            held.send(()).unwrap();
            thread::sleep(Duration::from_secs(3));
        })
        .unwrap();
    });
    holding.recv().unwrap();
    let started = Instant::now();
    let error = run(
        helper_command("quiet"),
        Limits::new(Duration::from_millis(200), 8192),
    )
    .unwrap_err();
    assert!(error.timed_out(), "{error}");
    // It gave up at its own deadline instead of waiting out the start in front of it.
    assert!(started.elapsed() < Duration::from_millis(2500), "{error}");
    blocker.join().unwrap();
}
