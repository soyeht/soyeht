//! The log records of a request and of a connection: when an operation started, how it ended,
//! and why a connection closed. A record names only an allowlisted method and the engine's
//! own identifiers; request ids and parameters can hold credentials and never enter it.

use crate::{
    dispatch::Outcome,
    errors::{self, Error},
    outbox::CloseReason,
};
use serde_json::Value;
use soyeht_platform::log::Line;
use std::time::Instant;

struct OperationTiming {
    started_at: chrono::DateTime<chrono::Utc>,
    finished_at: chrono::DateTime<chrono::Utc>,
    duration_ms: u128,
}

pub(crate) struct OperationTimer {
    operation_id: String,
    connection_id: String,
    method: &'static str,
    pane_id: Option<String>,
    started_at: chrono::DateTime<chrono::Utc>,
    started: Instant,
}

impl OperationTimer {
    pub(crate) fn start(connection: &str, method: &'static str, pane_id: Option<String>) -> Self {
        Self::start_with(
            connection,
            method,
            pane_id,
            chrono::Utc::now(),
            Instant::now(),
            emit_operation_record,
        )
    }

    pub(crate) fn start_with(
        connection: &str,
        method: &'static str,
        pane_id: Option<String>,
        started_at: chrono::DateTime<chrono::Utc>,
        started: Instant,
        emit: impl FnOnce(Line),
    ) -> Self {
        let timer = Self {
            operation_id: uuid::Uuid::new_v4().to_string(),
            connection_id: connection.to_owned(),
            method,
            pane_id,
            started_at,
            started,
        };
        emit(timer.started_record());
        timer
    }

    fn started_record(&self) -> Line {
        operation_started_record(
            &self.operation_id,
            &self.connection_id,
            self.method,
            self.pane_id.as_deref(),
            self.started_at,
        )
    }

    pub(crate) fn finish(self, outcome: &crate::errors::Result<Outcome>) {
        let (status, kind) = operation_status(outcome);
        self.finish_status(status, kind.as_ref());
    }

    pub(crate) fn finish_status(self, status: &str, kind: Option<&soyeht_protocol::ErrorKind>) {
        let finished_at = chrono::Utc::now();
        let duration_ms = self.started.elapsed().as_millis();
        self.finish_with(
            status,
            kind,
            finished_at,
            duration_ms,
            emit_operation_record,
        );
    }

    fn finish_with(
        self,
        status: &str,
        kind: Option<&soyeht_protocol::ErrorKind>,
        finished_at: chrono::DateTime<chrono::Utc>,
        duration_ms: u128,
        emit: impl FnOnce(Line),
    ) {
        emit(operation_record(
            &self.operation_id,
            &self.connection_id,
            self.method,
            self.pane_id.as_deref(),
            status,
            kind,
            &OperationTiming {
                started_at: self.started_at,
                finished_at,
                duration_ms,
            },
        ));
    }
}

fn operation_status(
    outcome: &crate::errors::Result<Outcome>,
) -> (&'static str, Option<soyeht_protocol::ErrorKind>) {
    match outcome {
        Ok(Outcome::Reply(_)) => ("reply", None),
        Ok(Outcome::Offload(_) | Outcome::Catalog { .. }) | Err(Error::Join(_)) => {
            ("worker_failed", None)
        }
        Ok(Outcome::Attachment { .. }) => ("attachment", None),
        Ok(Outcome::Queued) => ("queued", None),
        Ok(Outcome::Snapshot { .. }) => ("snapshot", None),
        Err(error) => ("error", Some(errors::kind(error))),
    }
}

fn emit_operation_record(record: Line) {
    record.emit();
}

pub(crate) fn logged_method(method: &str) -> &'static str {
    soyeht_protocol::METHODS
        .iter()
        .find(|entry| entry.0 == method)
        .map(|entry| entry.0)
        .unwrap_or("unknown")
}

fn operation_started_record(
    operation: &str,
    connection: &str,
    method: &str,
    pane_id: Option<&str>,
    started_at: chrono::DateTime<chrono::Utc>,
) -> Line {
    Line::new("engine.operation_started")
        .field("operationId", operation)
        .field("connectionId", connection)
        .field("method", logged_method(method))
        .field_opt("paneId", pane_id)
        .field("startedAt", timestamp_text(started_at))
}

fn timestamp_text(time: chrono::DateTime<chrono::Utc>) -> String {
    time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub(crate) fn connection_close_record(connection: &str, local: bool, reason: CloseReason) -> Line {
    Line::new("engine.connection_closed")
        .field("connectionId", connection)
        .field("transport", if local { "local" } else { "websocket" })
        .field("reason", reason.as_str())
}

fn operation_record(
    operation: &str,
    connection: &str,
    method: &str,
    pane_id: Option<&str>,
    outcome: &str,
    kind: Option<&soyeht_protocol::ErrorKind>,
    timing: &OperationTiming,
) -> Line {
    // Only an allowlisted method name is retained. Request IDs and parameters
    // may contain credentials or terminal bytes and never enter this record.
    Line::new("engine.operation")
        .field("operationId", operation)
        .field("connectionId", connection)
        .field("method", logged_method(method))
        .field_opt("paneId", pane_id)
        .field("outcome", outcome)
        .field_opt("kind", kind.map(errors::wire_name))
        .field("startedAt", timestamp_text(timing.started_at))
        .field("finishedAt", timestamp_text(timing.finished_at))
        .field("durationMs", timing.duration_ms)
}

pub(crate) fn operation_pane_id<'a>(method: &str, request: &'a Value) -> Option<&'a str> {
    let route = soyeht_protocol::PANE_ROUTE_METHODS
        .iter()
        .find_map(|(candidate, property)| (*candidate == method).then_some(*property))?;
    let pane_id = request.get("params")?.get(route)?.as_str()?;
    let parsed = uuid::Uuid::parse_str(pane_id).ok()?;
    (parsed.hyphenated().to_string() == pane_id).then_some(pane_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line as the JSON object the log carries.
    fn object(line: &Line) -> Value {
        serde_json::from_str(&line.json()).unwrap()
    }
    fn timestamp(value: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(value)
            .expect("valid test timestamp")
            .with_timezone(&chrono::Utc)
    }

    #[test]
    fn operation_log_keeps_identity_but_never_unknown_method_text() {
        let operation = uuid::Uuid::new_v4().to_string();
        let connection = uuid::Uuid::new_v4().to_string();
        let timing = OperationTiming {
            started_at: timestamp("2026-09-17T10:00:00.123Z"),
            finished_at: timestamp("2026-09-17T10:00:00.135Z"),
            duration_ms: 12,
        };
        let record = object(&operation_record(
            &operation,
            &connection,
            "untrusted-private-text",
            None,
            "error",
            Some(&soyeht_protocol::ErrorKind::InvalidParams),
            &timing,
        ));
        assert_eq!(record["operationId"], operation);
        assert_eq!(record["connectionId"], connection);
        assert_eq!(record["method"], "unknown");
        assert_eq!(record["kind"], "invalid_params");
        assert_eq!(record["startedAt"], "2026-09-17T10:00:00.123Z");
        assert_eq!(record["finishedAt"], "2026-09-17T10:00:00.135Z");
        assert_eq!(record["durationMs"], "12");
        assert!(!record.to_string().contains("untrusted-private-text"));
        let started = object(&operation_started_record(
            &operation,
            &connection,
            "untrusted-private-text",
            None,
            timing.started_at,
        ));
        assert_eq!(started["event"], "engine.operation_started");
        assert_eq!(started["method"], "unknown");
        assert!(!started.to_string().contains("untrusted-private-text"));
        let success = object(&operation_record(
            &operation,
            &connection,
            "pane.create",
            None,
            "reply",
            None,
            &timing,
        ));
        assert_eq!(success["method"], "pane.create");
        assert!(success["kind"].is_null());
    }

    #[test]
    fn operation_start_is_durable_without_a_finish_and_correlates_when_finished() {
        let started_at = timestamp("2026-09-17T10:00:00.123Z");
        let mut open_records = Vec::new();
        let open = OperationTimer::start_with(
            "connection-open",
            "model.list",
            None,
            started_at,
            Instant::now(),
            |record| open_records.push(object(&record)),
        );
        let open_id = open.operation_id.clone();
        drop(open);

        assert_eq!(open_records.len(), 1);
        assert_eq!(open_records[0]["event"], "engine.operation_started");
        assert_eq!(open_records[0]["operationId"], open_id);
        assert_eq!(open_records[0]["connectionId"], "connection-open");
        assert_eq!(open_records[0]["method"], "model.list");
        assert_eq!(open_records[0]["startedAt"], "2026-09-17T10:00:00.123Z");
        assert!(open_records[0].get("finishedAt").is_none());
        assert!(open_records[0].get("durationMs").is_none());
        assert!(open_records[0].get("outcome").is_none());

        let mut completed_records = Vec::new();
        let completed = OperationTimer::start_with(
            "connection-finished",
            "harness.list",
            None,
            started_at,
            Instant::now(),
            |record| completed_records.push(object(&record)),
        );
        let completed_id = completed.operation_id.clone();
        let outcome: crate::errors::Result<Outcome> = Ok(Outcome::Reply(serde_json::json!({})));
        let (status, kind) = operation_status(&outcome);
        completed.finish_with(
            status,
            kind.as_ref(),
            timestamp("2026-09-17T10:00:00.135Z"),
            12,
            |record| completed_records.push(object(&record)),
        );

        assert_eq!(completed_records.len(), 2);
        assert_eq!(completed_records[0]["event"], "engine.operation_started");
        assert_eq!(completed_records[1]["event"], "engine.operation");
        assert_eq!(completed_records[0]["operationId"], completed_id);
        assert_eq!(completed_records[1]["operationId"], completed_id);
        assert_eq!(completed_records[0]["connectionId"], "connection-finished");
        assert_eq!(completed_records[1]["connectionId"], "connection-finished");
        assert_eq!(completed_records[0]["method"], "harness.list");
        assert_eq!(completed_records[1]["method"], "harness.list");
        assert_eq!(
            completed_records[1]["finishedAt"],
            "2026-09-17T10:00:00.135Z"
        );
        assert_eq!(completed_records[1]["durationMs"], "12");
        assert_eq!(completed_records[1]["outcome"], "reply");
    }
    #[test]
    fn operation_log_adds_only_a_validated_pane_route() {
        let pane_id = uuid::Uuid::new_v4().to_string();
        let secret = "hook-capability-must-not-enter-the-log";
        let request = serde_json::json!({
            "method": "pane.stream.subscribe",
            "params": {"paneId":pane_id,"capability":secret},
        });
        let routed = operation_pane_id("pane.stream.subscribe", &request);
        let timing = OperationTiming {
            started_at: timestamp("2026-09-17T10:00:01.000Z"),
            finished_at: timestamp("2026-09-17T10:00:01.002Z"),
            duration_ms: 2,
        };
        let record = object(&operation_record(
            "operation",
            "connection",
            "pane.stream.subscribe",
            routed,
            "queued",
            None,
            &timing,
        ));
        assert_eq!(record["paneId"], pane_id);
        assert!(!record.to_string().contains(secret));

        let uppercase = serde_json::json!({
            "params": {"paneId":pane_id.to_uppercase(),"capability":secret},
        });
        assert!(operation_pane_id("pane.stream.subscribe", &uppercase).is_none());
        assert!(operation_pane_id("unknown", &request).is_none());

        let all_routes = serde_json::json!({
            "params": {"paneId":pane_id,"agentPaneId":pane_id,"targetPaneId":pane_id},
        });
        let expected_methods = soyeht_protocol::PANE_ROUTE_METHODS
            .iter()
            .map(|(method, _)| *method)
            .collect::<Vec<_>>();
        let routed_methods = soyeht_protocol::METHODS
            .iter()
            .filter_map(|(method, _, _, _)| operation_pane_id(method, &all_routes).map(|_| *method))
            .collect::<Vec<_>>();
        assert_eq!(routed_methods, expected_methods);

        let writing = serde_json::json!({
            "params": {"agentPaneId":pane_id},
        });
        assert_eq!(
            operation_pane_id("writing.variant.request", &writing),
            Some(pane_id.as_str())
        );

        let usage = serde_json::json!({
            "method": "usage.refresh",
            "params": {"paneId":pane_id},
        });
        assert!(operation_pane_id("usage.refresh", &usage).is_none());
    }
    #[test]
    fn connection_close_log_has_a_bounded_reason_and_no_transport_error_text() {
        let connection = uuid::Uuid::new_v4().to_string();
        let record = object(&connection_close_record(
            &connection,
            true,
            CloseReason::WriteError,
        ));
        assert_eq!(record["event"], "engine.connection_closed");
        assert_eq!(record["connectionId"], connection);
        assert_eq!(record["transport"], "local");
        assert_eq!(record["reason"], "write_error");
        assert_eq!(record.as_object().unwrap().len(), 4);
    }
}
