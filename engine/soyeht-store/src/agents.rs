//! Agent persistence uses the caller's transaction so public events, native
//! replies and runtime checkpoints commit together before any pipe write.
use crate::database::{Error, Result};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use soyeht_protocol::{PaneTranscriptResult, ProbeCapability, TranscriptMessage, limits};
use soyeht_protocol::{PromptItem, PromptStatus};

/// Prompt bodies are separate rows, not part of the frequently written ACP
/// checkpoint. A full waiting queue is never rewritten for each output chunk.
pub fn queue(db: &Connection, pane: &str) -> Result<Vec<PromptItem>> {
    let mut statement =
        db.prepare("SELECT payload FROM agent_prompt_queue WHERE pane_id=?1 ORDER BY ordinal")?;
    let rows = statement.query_map([pane], |r| r.get::<_, String>(0))?;
    rows.map(|raw| Ok(serde_json::from_str(&raw?)?)).collect()
}

pub fn enqueue_prompt(tx: &Transaction<'_>, pane: &str, item: &PromptItem) -> Result<()> {
    if item.text.is_empty()
        || item.text.len() > limits::PROMPT_BYTES
        || item.status != PromptStatus::Queued
    {
        return Err(Error::Invalid("invalid queued prompt"));
    }
    let raw = serde_json::to_string(item)?;
    let prior: Option<(String, String)> = tx
        .query_row(
            "SELECT pane_id,payload FROM agent_prompt_queue WHERE prompt_id=?1",
            [&item.prompt_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((owner, previous)) = prior {
        return if owner == pane && previous == raw {
            Ok(())
        } else {
            Err(Error::IdempotencyConflict)
        };
    }
    let count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM agent_prompt_queue WHERE pane_id=?1",
        [pane],
        |r| r.get(0),
    )?;
    if count >= limits::PROMPT_QUEUE_COUNT as i64 {
        return Err(Error::Invalid("prompt queue is full"));
    }
    tx.execute(
        "INSERT INTO agent_prompt_queue(prompt_id,pane_id,payload) VALUES (?1,?2,?3)",
        params![item.prompt_id, pane, raw],
    )?;
    Ok(())
}

pub fn remove_prompt(tx: &Transaction<'_>, pane: &str, prompt: &str) -> Result<bool> {
    Ok(tx.execute(
        "DELETE FROM agent_prompt_queue WHERE pane_id=?1 AND prompt_id=?2",
        params![pane, prompt],
    )? == 1)
}

pub fn runtime<T: DeserializeOwned>(db: &Connection, pane: &str) -> Result<Option<T>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM agent_runtime WHERE pane_id=?1",
            [pane],
            |row| row.get(0),
        )
        .optional()?;
    raw.map(|raw| Ok(serde_json::from_str(&raw)?)).transpose()
}

pub fn save_runtime<T: Serialize>(
    tx: &Transaction<'_>,
    pane: &str,
    generation: u32,
    value: &T,
) -> Result<()> {
    let raw = serde_json::to_string(value)?;
    if raw.len() > 16 * limits::JSON_BYTES || generation == 0 {
        return Err(Error::Invalid("agent runtime exceeds its limit"));
    }
    let changed = tx.execute("INSERT INTO agent_runtime(pane_id,generation,payload) VALUES (?1,?2,?3) ON CONFLICT(pane_id) DO UPDATE SET generation=excluded.generation,payload=excluded.payload WHERE agent_runtime.generation<=excluded.generation", params![pane,generation,raw])?;
    if changed != 1 {
        return Err(Error::Invalid("stale agent generation"));
    }
    Ok(())
}

fn generation_matches(db: &Connection, pane: &str, generation: u32) -> Result<()> {
    let matches: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_runtime WHERE pane_id=?1 AND generation=?2)",
        params![pane, generation],
        |r| r.get(0),
    )?;
    if !matches {
        return Err(Error::Invalid("stale agent generation"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outgoing {
    pub operation_key: String,
    pub pane_id: String,
    pub generation: u32,
    pub instance_id: String,
    pub supervisor_id: String,
    pub bytes: Vec<u8>,
    pub accepted_bytes: u32,
    pub attempt: u32,
}

pub fn enqueue(tx: &Transaction<'_>, value: &Outgoing) -> Result<()> {
    generation_matches(tx, &value.pane_id, value.generation)?;
    if value.accepted_bytes != 0 || value.attempt != 0 {
        return Err(Error::Invalid("new outbox operation has delivery progress"));
    }
    if value.bytes.last() != Some(&b'\n')
        || value.bytes.len() > limits::ACP_MESSAGE_BYTES + 1
        || value.bytes[..value.bytes.len() - 1].contains(&b'\n')
    {
        return Err(Error::Invalid("outbox needs one complete ACP line"));
    }
    let message: Value = serde_json::from_slice(&value.bytes[..value.bytes.len() - 1])?;
    if !message.is_object() || message["jsonrpc"] != "2.0" {
        return Err(Error::Invalid("outbox needs a JSON-RPC envelope"));
    }
    if let Some(mut prior) = outgoing(tx, &value.operation_key)? {
        prior.accepted_bytes = 0;
        prior.attempt = 0;
        return if prior == *value {
            Ok(())
        } else {
            Err(Error::IdempotencyConflict)
        };
    }
    let (count, bytes): (i64, i64) = tx.query_row(
        "SELECT COUNT(*),COALESCE(SUM(length(bytes)),0) FROM agent_outbox WHERE pane_id=?1",
        [&value.pane_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if count >= 128 || bytes + value.bytes.len() as i64 > limits::CONNECTION_QUEUE_BYTES as i64 {
        return Err(Error::Invalid("agent outbox is full"));
    }
    tx.execute("INSERT INTO agent_outbox(operation_key,pane_id,generation,instance_id,supervisor_id,bytes) VALUES (?1,?2,?3,?4,?5,?6)",params![value.operation_key,value.pane_id,value.generation,value.instance_id,value.supervisor_id,value.bytes])?;
    Ok(())
}

fn outgoing_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Outgoing> {
    Ok(Outgoing {
        operation_key: row.get(0)?,
        pane_id: row.get(1)?,
        generation: row.get(2)?,
        instance_id: row.get(3)?,
        supervisor_id: row.get(4)?,
        bytes: row.get(5)?,
        accepted_bytes: row.get(6)?,
        attempt: row.get(7)?,
    })
}
pub fn outgoing(db: &Connection, key: &str) -> Result<Option<Outgoing>> {
    Ok(db.query_row("SELECT operation_key,pane_id,generation,instance_id,supervisor_id,bytes,accepted_bytes,attempt FROM agent_outbox WHERE operation_key=?1",[key],outgoing_row).optional()?)
}
pub fn outbox(db: &Connection, pane: &str) -> Result<Vec<Outgoing>> {
    let mut statement = db.prepare("SELECT operation_key,pane_id,generation,instance_id,supervisor_id,bytes,accepted_bytes,attempt FROM agent_outbox WHERE pane_id=?1 ORDER BY ordinal")?;
    Ok(statement
        .query_map([pane], outgoing_row)?
        .collect::<rusqlite::Result<_>>()?)
}

/// Persist each partial write before confirming its ptyd receipt. A zero-byte
/// write still advances the attempt, so its deduplicated receipt cannot stall
/// the next try forever. The byte offset advances only by the accepted count.
pub fn advance(tx: &Transaction<'_>, prior: &Outgoing, accepted: usize) -> Result<Outgoing> {
    generation_matches(tx, &prior.pane_id, prior.generation)?;
    if outgoing(tx, &prior.operation_key)?.as_ref() != Some(prior) {
        return Err(Error::IdempotencyConflict);
    }
    if accepted > limits::TERMINAL_PAYLOAD_BYTES
        || accepted > prior.bytes.len() - prior.accepted_bytes as usize
    {
        return Err(Error::Invalid("invalid native partial-write receipt"));
    }
    let mut next = prior.clone();
    next.accepted_bytes += accepted as u32;
    next.attempt = next
        .attempt
        .checked_add(1)
        .ok_or(Error::Invalid("native write attempts exhausted"))?;
    tx.execute(
        "UPDATE agent_outbox SET accepted_bytes=?1,attempt=?2 WHERE operation_key=?3",
        params![next.accepted_bytes, next.attempt, next.operation_key],
    )?;
    Ok(next)
}

/// Call only after ptyd accepted these exact bytes for the bound process.
/// The caller confirms ptyd's input receipt only AFTER this commit.
pub fn delivered(tx: &Transaction<'_>, value: &Outgoing) -> Result<()> {
    if value.accepted_bytes as usize != value.bytes.len() {
        return Err(Error::Invalid("native message is not fully delivered"));
    }
    if let Some(prior) = outgoing(tx, &value.operation_key)? {
        if prior != *value {
            return Err(Error::IdempotencyConflict);
        }
        tx.execute(
            "DELETE FROM agent_outbox WHERE operation_key=?1",
            [&value.operation_key],
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeRequest {
    pub request_id: String,
    pub pane_id: String,
    pub generation: u32,
    pub source_seq: u64,
    pub native_id: Value,
    pub method: String,
    pub params: Value,
    pub public: Value,
    pub expires_ms: i64,
    pub result: Option<Value>,
}

fn request_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NativeRequest> {
    fn json(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Value> {
        let raw: String = row.get(index)?;
        serde_json::from_str(&raw).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })
    }
    let seq: [u8; 8] = row
        .get::<_, Vec<u8>>(3)?
        .try_into()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(NativeRequest {
        request_id: row.get(0)?,
        pane_id: row.get(1)?,
        generation: row.get(2)?,
        source_seq: u64::from_be_bytes(seq),
        native_id: json(row, 4)?,
        method: row.get(5)?,
        params: json(row, 6)?,
        public: json(row, 7)?,
        expires_ms: row.get(8)?,
        result: if row.get::<_, Option<String>>(9)?.is_some() {
            Some(json(row, 9)?)
        } else {
            None
        },
    })
}
const REQUEST_COLUMNS: &str =
    "request_id,pane_id,generation,source_seq,native_id,method,params,public,expires_ms,result";
pub fn request(db: &Connection, id: &str) -> Result<Option<NativeRequest>> {
    Ok(db
        .query_row(
            &format!("SELECT {REQUEST_COLUMNS} FROM agent_requests WHERE request_id=?1"),
            [id],
            request_row,
        )
        .optional()?)
}
pub fn request_at(
    db: &Connection,
    pane: &str,
    generation: u32,
    seq: u64,
) -> Result<Option<NativeRequest>> {
    Ok(db.query_row(&format!("SELECT {REQUEST_COLUMNS} FROM agent_requests WHERE pane_id=?1 AND generation=?2 AND source_seq=?3"),params![pane,generation,seq.to_be_bytes().as_slice()],request_row).optional()?)
}
pub fn open_requests(db: &Connection, pane: &str) -> Result<Vec<NativeRequest>> {
    let mut statement = db.prepare(&format!("SELECT {REQUEST_COLUMNS} FROM agent_requests WHERE pane_id=?1 AND result IS NULL ORDER BY source_seq"))?;
    Ok(statement
        .query_map([pane], request_row)?
        .collect::<rusqlite::Result<_>>()?)
}

/// Returns the previously persisted identity when an already-seen native
/// request is replayed. Its deadline and engine request ID never change.
pub fn receive(tx: &Transaction<'_>, value: &NativeRequest) -> Result<NativeRequest> {
    generation_matches(tx, &value.pane_id, value.generation)?;
    if let Some(prior) = request_at(tx, &value.pane_id, value.generation, value.source_seq)? {
        if prior.native_id != value.native_id
            || prior.method != value.method
            || prior.params != value.params
        {
            return Err(Error::IdempotencyConflict);
        }
        return Ok(prior);
    }
    if value.result.is_some()
        || !(value.native_id.is_null() || value.native_id.is_string() || value.native_id.is_i64())
    {
        return Err(Error::Invalid("invalid native request identity"));
    }
    let size = serde_json::to_vec(value)?.len();
    if serde_json::to_vec(&value.public)?.len() > limits::JSON_BYTES / 2 {
        return Err(Error::Invalid(
            "public request exceeds the snapshot item limit",
        ));
    }
    let (count, bytes): (i64, i64) = tx.query_row("SELECT COUNT(*),COALESCE(SUM(length(CAST(params AS BLOB))+length(CAST(public AS BLOB))),0) FROM agent_requests WHERE pane_id=?1 AND result IS NULL",[&value.pane_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if count >= 128
        || bytes + size as i64 > 16 * limits::JSON_BYTES as i64
        || size > 2 * limits::JSON_BYTES
    {
        return Err(Error::Invalid("pending native requests exceed their limit"));
    }
    tx.execute("INSERT INTO agent_requests(request_id,pane_id,generation,source_seq,native_id,method,params,public,expires_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![value.request_id,value.pane_id,value.generation,value.source_seq.to_be_bytes().as_slice(),serde_json::to_string(&value.native_id)?,value.method,serde_json::to_string(&value.params)?,serde_json::to_string(&value.public)?,value.expires_ms])?;
    Ok(value.clone())
}

#[derive(Debug, PartialEq)]
pub struct Decision {
    pub first: bool,
    pub result: Value,
}
/// First answer wins inside the same transaction as its native response and
/// public event. A rollback restores the open request, including its deadline.
pub fn answer(tx: &Transaction<'_>, id: &str, result: &Value) -> Result<Decision> {
    let prior = request(tx, id)?.ok_or(Error::Invalid("native request not found"))?;
    if let Some(result) = prior.result {
        return Ok(Decision {
            first: false,
            result,
        });
    }
    generation_matches(tx, &prior.pane_id, prior.generation)?;
    tx.execute(
        "UPDATE agent_requests SET result=?1 WHERE request_id=?2 AND result IS NULL",
        params![serde_json::to_string(result)?, id],
    )?;
    Ok(Decision {
        first: true,
        result: result.clone(),
    })
}

pub fn save_message(tx: &Transaction<'_>, pane: &str, message: &TranscriptMessage) -> Result<()> {
    generation_matches(tx, pane, message.generation)?;
    let seq = message
        .seq
        .parse::<u64>()
        .map_err(|_| Error::Invalid("invalid transcript sequence"))?;
    let raw = serde_json::to_string(message)?;
    if raw.len() > limits::TRANSCRIPT_MESSAGE_BYTES {
        return Err(Error::Invalid("transcript message exceeds its byte limit"));
    }
    let prior: Option<String> = tx
        .query_row(
            "SELECT payload FROM agent_transcript WHERE pane_id=?1 AND seq=?2",
            params![pane, seq.to_be_bytes().as_slice()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(prior) = prior {
        let prior: TranscriptMessage = serde_json::from_str(&prior)?;
        if prior.local_id != message.local_id
            || prior.generation != message.generation
            || prior.kind != message.kind
            || prior.role != message.role
            || prior.prompt_id != message.prompt_id
            || (prior.complete && prior != *message)
        {
            return Err(Error::Invalid(
                "transcript identity or completed content changed",
            ));
        }
    }
    tx.execute("INSERT INTO agent_transcript(pane_id,seq,local_id,generation,payload,bytes) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(pane_id,seq) DO UPDATE SET payload=excluded.payload,bytes=excluded.bytes",params![pane,seq.to_be_bytes().as_slice(),message.local_id,message.generation,raw,raw.len() as i64])?;
    let mut bytes: i64 = tx.query_row(
        "SELECT COALESCE(SUM(bytes),0) FROM agent_transcript WHERE pane_id=?1",
        [pane],
        |r| r.get(0),
    )?;
    while bytes > limits::PANE_RETENTION_BYTES as i64 {
        let (old, size): (Vec<u8>, i64) = tx.query_row(
            "SELECT seq,bytes FROM agent_transcript WHERE pane_id=?1 ORDER BY seq LIMIT 1",
            [pane],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        tx.execute(
            "DELETE FROM agent_transcript WHERE pane_id=?1 AND seq=?2",
            params![pane, old],
        )?;
        bytes -= size;
    }
    Ok(())
}

/// Newest page, returned in display order; `before` is exclusive. Eight-byte
/// big-endian keys preserve all u64 sequences without signed SQLite overflow.
pub fn transcript(
    db: &Connection,
    pane: &str,
    before: Option<u64>,
    limit: usize,
) -> Result<PaneTranscriptResult> {
    if limit == 0 || limit > limits::TRANSCRIPT_PAGE_MESSAGES {
        return Err(Error::Invalid("invalid transcript page limit"));
    }
    let mut statement=db.prepare("SELECT payload FROM agent_transcript WHERE pane_id=?1 AND (?2 IS NULL OR seq<?2) ORDER BY seq DESC LIMIT ?3")?;
    let rows = statement.query_map(
        params![
            pane,
            before.map(|n| n.to_be_bytes().to_vec()),
            (limit + 1) as i64
        ],
        |r| r.get::<_, String>(0),
    )?;
    let mut items = Vec::new();
    let mut bytes = 0;
    let mut more = false;
    for raw in rows {
        let raw = raw?;
        if items.len() == limit || bytes + raw.len() + 1 > limits::TRANSCRIPT_PAGE_BYTES - 256 {
            more = true;
            break;
        }
        bytes += raw.len() + 1;
        items.push(serde_json::from_str::<TranscriptMessage>(&raw)?);
    }
    items.reverse();
    let next_cursor = more.then(|| {
        items
            .first()
            .expect("one bounded message fits a page")
            .seq
            .clone()
    });
    Ok(PaneTranscriptResult { items, next_cursor })
}

pub fn probe(db: &Connection, fingerprint: &str, method: &str) -> Result<Option<ProbeCapability>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM agent_probes WHERE fingerprint=?1 AND method=?2",
            params![fingerprint, method],
            |r| r.get(0),
        )
        .optional()?;
    raw.map(|raw| Ok(serde_json::from_str(&raw)?)).transpose()
}
pub fn save_probe(tx: &Transaction<'_>, fingerprint: &str, value: &ProbeCapability) -> Result<()> {
    tx.execute("INSERT INTO agent_probes(fingerprint,method,payload) VALUES (?1,?2,?3) ON CONFLICT(fingerprint,method) DO UPDATE SET payload=excluded.payload",params![fingerprint,value.method,serde_json::to_string(value)?])?;
    Ok(())
}
