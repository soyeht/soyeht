use super::Exit;
use soyeht_platform::process::windows::{Console, Job, Spawned};
use std::{io, os::windows::io::OwnedHandle, sync::Arc, time::Duration};
use windows_sys::Win32::System::Threading::INFINITE;

/// How long ending a child waits for its processes to be gone. They are gone in milliseconds;
/// the bound only keeps a stuck process from holding the caller's lock for good.
const END_WAIT: Duration = Duration::from_secs(2);

/// A child in its own job, with the pseudo console it runs on when it has one. The job holds
/// everything the child starts, so ending it ends the whole tree.
pub struct Child {
    spawned: Spawned,
    pid: u32,
    console: Option<Arc<Console>>,
    exit: Option<Exit>,
}

impl Child {
    pub(crate) fn new(
        process: OwnedHandle,
        pid: u32,
        job: Job,
        console: Option<Arc<Console>>,
    ) -> Self {
        Self {
            spawned: Spawned::new(process, job),
            pid,
            console,
            exit: None,
        }
    }

    pub fn id(&self) -> u32 {
        self.pid
    }

    pub fn try_wait(&mut self) -> io::Result<Option<Exit>> {
        self.wait_for(0)
    }

    pub fn wait(&mut self) -> io::Result<Exit> {
        loop {
            if let Some(exit) = self.wait_for(INFINITE)? {
                return Ok(exit);
            }
        }
    }

    fn wait_for(&mut self, milliseconds: u32) -> io::Result<Option<Exit>> {
        if let Some(exit) = self.exit {
            return Ok(Some(exit));
        }
        let Some(code) = self.spawned.wait_exit(milliseconds)? else {
            return Ok(None);
        };
        // A terminal's output pipe stays open until its console closes; closing it now ends the
        // pipe once the program's last output has been read. The caller holds the lock around
        // this child, and the close waits for the reader, so it happens on another thread.
        if let Some(console) = &self.console {
            console.close_in_background();
        }
        let exit = Exit(Some(code as i32));
        self.exit = Some(exit);
        Ok(Some(exit))
    }

    /// Ends the child and everything it started.
    pub fn kill_group(&self) {
        if let Err(error) = self.spawned.kill_tree(END_WAIT) {
            eprintln!(
                "{{\"event\":\"ptyd.kill_tree_failed\",\"error\":\"{:?}\"}}",
                error.kind()
            );
        }
    }

    /// Ends the child and everything it started.
    pub fn kill_tree(&self) {
        self.kill_group();
    }

    /// Hangs up the child's terminal, as closing its window would. A child without a terminal
    /// is ended, because a hang-up has nothing to reach.
    pub fn request_stop(&self) {
        match &self.console {
            Some(console) => console.close_in_background(),
            None => self.kill_group(),
        }
    }
}
