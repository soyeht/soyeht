//! How a service is kept running, the same on every system where the system allows it.

/// Seconds the system waits before it starts a program again after it ended: launchd's
/// `ThrottleInterval` and systemd's `RestartSec`. Short, since a client that finds no engine
/// shows the person an error (ADR 0108); a program that fails at once still starts only once a
/// second.
pub(crate) const RESTART_DELAY_SECONDS: u32 = 1;

/// Task Scheduler cannot wait less than a minute between restarts of a failed task.
pub(crate) const WINDOWS_RESTART_INTERVAL: &str = "PT1M";

/// How many times Task Scheduler restarts a failed task, the most it takes.
pub(crate) const WINDOWS_RESTART_COUNT: u32 = 999;

/// Task Scheduler priority 5 is a normal process priority; its default, 7, is below normal.
pub(crate) const WINDOWS_PRIORITY: u32 = 5;

/// What the Mac lists as the service's workload: it is started for a person at the screen.
pub(crate) const LAUNCHD_PROCESS_TYPE: &str = "Interactive";

/// The folder of the data directory that keeps the copy of a Windows task's definition that was
/// handed to `schtasks`.
pub(crate) const TASK_COPY_FOLDER: &str = "service";

/// How long to wait before each new try of `launchctl bootstrap` that failed because launchd was
/// still removing the job a `bootout` ended: five tries in all, about two seconds in total.
pub(crate) const LAUNCHD_RELOAD_BACKOFF: [std::time::Duration; 4] = [
    std::time::Duration::from_millis(200),
    std::time::Duration::from_millis(400),
    std::time::Duration::from_millis(600),
    std::time::Duration::from_millis(800),
];
