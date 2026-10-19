use std::collections::{BTreeMap, BTreeSet};

use soyeht_protocol::{LocalServer, limits};

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CwdObservation {
    pub pid: u32,
    pub cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerTitle {
    pub pid: u32,
    pub port: u16,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Listener {
    pid: u32,
    command: String,
    port: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalServerScan {
    servers: Vec<LocalServer>,
}

impl LocalServerScan {
    pub fn parse_lsof(
        listener_output: &[u8],
        cwd_output: &[u8],
        titles: &[ServerTitle],
    ) -> Result<Self> {
        let cwd_observations = parse_cwds(cwd_output)?;
        Self::parse(listener_output, &cwd_observations, titles)
    }

    pub fn listener_pids(listener_output: &[u8]) -> Result<Vec<u32>> {
        let mut listeners = parse_listeners(listener_output)?;
        listeners.sort_by_key(|listener| (listener.port, listener.pid));
        let mut pids = BTreeSet::new();
        for listener in listeners {
            pids.insert(listener.pid);
            if pids.len() == limits::LOCAL_SERVER_ENTRIES {
                break;
            }
        }
        Ok(pids.into_iter().collect())
    }

    pub fn parse(
        listener_output: &[u8],
        cwd_observations: &[CwdObservation],
        titles: &[ServerTitle],
    ) -> Result<Self> {
        let mut listeners = parse_listeners(listener_output)?;
        listeners.sort_by_key(|listener| (listener.port, listener.pid));
        if cwd_observations.iter().any(|item| {
            item.pid == 0
                || item.cwd.is_empty()
                || item.cwd.len() > 4096
                || item.cwd.chars().any(char::is_control)
        }) || titles.iter().any(|item| {
            item.pid == 0
                || item.port == 0
                || item.title.len() > 1024
                || item.title.chars().any(char::is_control)
        }) {
            return Err(Error::InvalidPortRecord);
        }
        let cwd_by_pid = cwd_observations
            .iter()
            .map(|item| (item.pid, item.cwd.as_str()))
            .collect::<BTreeMap<_, _>>();
        let title_by_socket = titles
            .iter()
            .map(|item| ((item.pid, item.port), item.title.as_str()))
            .collect::<BTreeMap<_, _>>();
        let mut unique = BTreeSet::new();
        let mut servers = Vec::new();
        for listener in listeners {
            if !unique.insert((listener.port, listener.pid)) {
                continue;
            }
            let Some(cwd) = cwd_by_pid.get(&listener.pid) else {
                continue;
            };
            servers.push(LocalServer {
                port: u32::from(listener.port),
                pid: listener.pid,
                command: listener.command,
                cwd: (*cwd).to_owned(),
                title: title_by_socket
                    .get(&(listener.pid, listener.port))
                    .map(|value| (*value).to_owned()),
            });
            if servers.len() == limits::LOCAL_SERVER_ENTRIES {
                break;
            }
        }
        Ok(Self { servers })
    }

    pub fn servers(&self) -> &[LocalServer] {
        &self.servers
    }

    pub fn into_servers(self) -> Vec<LocalServer> {
        self.servers
    }
}

fn parse_cwds(output: &[u8]) -> Result<Vec<CwdObservation>> {
    if output.len() > limits::PORT_SCAN_BYTES {
        return Err(Error::PortScanTooLarge);
    }
    let text = std::str::from_utf8(output).map_err(|_| Error::InvalidPortRecord)?;
    let mut pid = None;
    let mut observations = Vec::new();
    for field in text.split(['\0', '\n']).filter(|field| !field.is_empty()) {
        match field.as_bytes().first() {
            Some(b'p') => pid = Some(parse_u32(&field[1..])?),
            Some(b'n') => observations.push(CwdObservation {
                pid: pid.ok_or(Error::InvalidPortRecord)?,
                cwd: checked_text(&field[1..])?,
            }),
            _ => {}
        }
    }
    Ok(observations)
}

fn parse_listeners(output: &[u8]) -> Result<Vec<Listener>> {
    if output.len() > limits::PORT_SCAN_BYTES {
        return Err(Error::PortScanTooLarge);
    }
    let text = std::str::from_utf8(output).map_err(|_| Error::InvalidPortRecord)?;
    let mut pid = None;
    let mut command = None;
    let mut listeners = Vec::new();
    for field in text.split(['\0', '\n']).filter(|field| !field.is_empty()) {
        match field.as_bytes().first() {
            Some(b'p') => {
                pid = Some(parse_u32(&field[1..])?);
                command = None;
            }
            Some(b'c') => command = Some(checked_text(&field[1..])?),
            Some(b'n') => {
                let current_pid = pid.ok_or(Error::InvalidPortRecord)?;
                let current_command = command.clone().ok_or(Error::InvalidPortRecord)?;
                let port = parse_port(&field[1..])?;
                if is_soyeht_process(&current_command) {
                    continue;
                }
                listeners.push(Listener {
                    pid: current_pid,
                    command: current_command,
                    port,
                });
            }
            _ => {}
        }
    }
    Ok(listeners)
}

/// Soyeht's own programs (the engine, the supervisor, the command line, the app) listen on
/// ports too, but they are not a project the person is working on, so they are never offered.
/// `lsof` shortens a command to nine characters, which still holds the prefix.
fn is_soyeht_process(command: &str) -> bool {
    command
        .as_bytes()
        .get(..SOYEHT_PROCESS_PREFIX.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(SOYEHT_PROCESS_PREFIX.as_bytes()))
}

const SOYEHT_PROCESS_PREFIX: &str = "soyeht";

fn parse_u32(value: &str) -> Result<u32> {
    value
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or(Error::InvalidPortRecord)
}

fn checked_text(value: &str) -> Result<String> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(Error::InvalidPortRecord);
    }
    Ok(value.to_owned())
}

fn parse_port(value: &str) -> Result<u16> {
    let value = value.strip_suffix(" (LISTEN)").unwrap_or(value);
    let port = value
        .rsplit_once(':')
        .map(|(_, port)| port)
        .unwrap_or(value);
    port.parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .ok_or(Error::InvalidPortRecord)
}
