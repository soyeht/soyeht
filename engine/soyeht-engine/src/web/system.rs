use crate::ports::{PortScanObservation, PortScanPort};
use soyeht_protocol::limits;
use soyeht_web::LocalServerScan;
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const SCAN_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

pub struct SystemPortScan {
    program: PathBuf,
}

impl Default for SystemPortScan {
    fn default() -> Self {
        Self {
            program: "/usr/sbin/lsof".into(),
        }
    }
}

impl PortScanPort for SystemPortScan {
    fn scan(&mut self) -> io::Result<PortScanObservation> {
        let listeners = run_lsof(
            &self.program,
            &["-nP", "-a", "-iTCP", "-sTCP:LISTEN", "-F0pcn"],
        )?;
        let pids = LocalServerScan::listener_pids(&listeners)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let directories = if pids.is_empty() {
            Vec::new()
        } else {
            let list = pids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            run_lsof(
                &self.program,
                &["-nP", "-a", "-p", &list, "-d", "cwd", "-F0pn"],
            )?
        };
        Ok(PortScanObservation {
            listeners,
            directories,
        })
    }
}

fn run_lsof(program: &Path, arguments: &[&str]) -> io::Result<Vec<u8>> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            "port scanner stdout is unavailable",
        )
    })?;
    let reader = thread::spawn(move || read_bounded(stdout));
    let status = wait_child(&mut child, Instant::now() + SCAN_TIMEOUT)?;
    let output = reader.join().map_err(|_| {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            "port scanner output worker failed",
        )
    })??;
    if status.success() || output.is_empty() {
        Ok(output)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "port scanner failed",
        ))
    }
}

fn wait_child(child: &mut Child, deadline: Instant) -> io::Result<std::process::ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "port scanner timed out",
            ));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn read_bounded(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(count) > limits::PORT_SCAN_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "port scanner output exceeds its byte budget",
            ));
        }
        output.extend_from_slice(&chunk[..count]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    use uuid::Uuid;

    fn script(body: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("soyeht-port-scan-{}", Uuid::new_v4()));
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[test]
    fn system_adapter_uses_two_bounded_machine_readable_observations() {
        let program = script(
            r#"if [ "$3" = "-p" ]; then
  printf 'p17\0fcwd\0n/tmp/site\0'
else
  printf 'p17\0cnode\0n127.0.0.1:5173\0'
fi"#,
        );
        let mut adapter = SystemPortScan {
            program: program.clone(),
        };
        let observation = adapter.scan().unwrap();
        let scan =
            LocalServerScan::parse_lsof(&observation.listeners, &observation.directories, &[])
                .unwrap();
        assert_eq!(scan.servers()[0].cwd, "/tmp/site");
        fs::remove_file(program).unwrap();
    }
}
