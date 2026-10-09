use crate::ports::{PortScanObservation, PortScanPort};
use soyeht_protocol::limits;
use soyeht_web::LocalServerScan;
use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const SCAN_TIMEOUT: Duration = Duration::from_secs(5);

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

/// Runs `lsof` off the Core lock with its deadline and byte budget.
fn run_lsof(program: &Path, arguments: &[&str]) -> io::Result<Vec<u8>> {
    let mut command = Command::new(program);
    command.args(arguments);
    let output = soyeht_process::run(
        command,
        soyeht_process::Limits::new(SCAN_TIMEOUT, limits::PORT_SCAN_BYTES),
    )
    .map_err(|error| match error {
        soyeht_process::Error::Spawn { source, .. } => source,
        soyeht_process::Error::TimedOut { .. } => {
            io::Error::new(io::ErrorKind::TimedOut, "port scanner timed out")
        }
        soyeht_process::Error::OutputTooLarge { .. } => io::Error::new(
            io::ErrorKind::InvalidData,
            "port scanner output exceeds its byte budget",
        ),
        other => io::Error::other(other.to_string()),
    })?;
    if output.success() || output.stdout.is_empty() {
        Ok(output.stdout)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "port scanner failed",
        ))
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
