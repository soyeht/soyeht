use std::collections::VecDeque;

use serde::Serialize;
use soyeht_protocol::{WebConsoleEntry, WebConsoleLevel, WebNetworkEntry, limits};

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingPage<T> {
    pub entries: Vec<T>,
    pub next: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConsoleMetadata {
    pub level: WebConsoleLevel,
    pub text: String,
    pub source: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkMetadata {
    pub method: String,
    pub url: String,
    pub status: u32,
    pub duration_ms: u32,
    pub bytes: u64,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Sequenced<T> {
    sequence: u64,
    value: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Ring<T> {
    capacity: usize,
    next: u64,
    entries: VecDeque<Sequenced<T>>,
}

impl<T: Clone> Ring<T> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            next: 1,
            entries: VecDeque::with_capacity(capacity),
        }
    }

    fn push(&mut self, value: T) -> Result<u64> {
        let sequence = self.next;
        self.next = self.next.checked_add(1).ok_or(Error::SequenceExhausted)?;
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(Sequenced { sequence, value });
        Ok(sequence)
    }

    fn page(&self, since: Option<u64>) -> (u64, bool, Vec<Sequenced<T>>) {
        let requested = since.unwrap_or(0);
        let first = self
            .entries
            .front()
            .map_or(self.next, |entry| entry.sequence);
        let truncated = requested.saturating_add(1) < first;
        let entries = self
            .entries
            .iter()
            .filter(|entry| entry.sequence > requested)
            .cloned()
            .collect();
        (requested, truncated, entries)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConsoleRing(Ring<ConsoleMetadata>);

impl Default for ConsoleRing {
    fn default() -> Self {
        Self(Ring::new(limits::WEB_CONSOLE_ENTRIES))
    }
}

impl ConsoleRing {
    pub fn push(&mut self, metadata: ConsoleMetadata) -> Result<u64> {
        validate_console(&metadata)?;
        self.0.push(metadata)
    }

    pub fn page(&self, since: Option<u64>) -> RingPage<WebConsoleEntry> {
        let (requested, truncated, entries) = self.0.page(since);
        bounded_page(requested, truncated, entries, |entry| WebConsoleEntry {
            sequence: entry.sequence.to_string(),
            level: entry.value.level,
            text: entry.value.text,
            source: entry.value.source,
            at: entry.value.at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkRing(Ring<NetworkMetadata>);

impl Default for NetworkRing {
    fn default() -> Self {
        Self(Ring::new(limits::WEB_NETWORK_ENTRIES))
    }
}

impl NetworkRing {
    pub fn push(&mut self, metadata: NetworkMetadata) -> Result<u64> {
        validate_network(&metadata)?;
        self.0.push(metadata)
    }

    pub fn page(&self, since: Option<u64>) -> RingPage<WebNetworkEntry> {
        let (requested, truncated, entries) = self.0.page(since);
        bounded_page(requested, truncated, entries, |entry| WebNetworkEntry {
            sequence: entry.sequence.to_string(),
            method: entry.value.method,
            url: entry.value.url,
            status: entry.value.status,
            duration_ms: entry.value.duration_ms,
            bytes: entry.value.bytes.to_string(),
            at: entry.value.at,
        })
    }
}

fn validate_console(item: &ConsoleMetadata) -> Result<()> {
    if item.text.len() <= 16_384 && item.source.len() <= 4096 && item.at.len() <= 64 {
        Ok(())
    } else {
        Err(Error::InvalidConsoleEntry)
    }
}

fn validate_network(item: &NetworkMetadata) -> Result<()> {
    let valid = !item.method.is_empty()
        && item.method.len() <= 32
        && !item.url.is_empty()
        && item.url.len() <= 4096
        && item.status <= 999
        && item.at.len() <= 64;
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidNetworkEntry)
    }
}

fn bounded_page<T, U>(
    requested: u64,
    mut truncated: bool,
    entries: Vec<Sequenced<T>>,
    convert: impl Fn(Sequenced<T>) -> U,
) -> RingPage<U>
where
    U: Serialize,
{
    let mut output = Vec::new();
    let mut bytes = 2_usize;
    let mut next = requested;
    for entry in entries {
        let sequence = entry.sequence;
        let value = convert(entry);
        let value_bytes =
            serde_json::to_vec(&value).map_or(limits::JSON_BYTES, |value| value.len());
        let separator = usize::from(!output.is_empty());
        if bytes.saturating_add(separator).saturating_add(value_bytes) > limits::JSON_BYTES - 4096 {
            truncated = true;
            break;
        }
        bytes = bytes.saturating_add(separator).saturating_add(value_bytes);
        next = sequence;
        output.push(value);
    }
    RingPage {
        entries: output,
        next,
        truncated,
    }
}
