//! Version-three migration for protocol-owned fields. User text is never translated.
use crate::database::Result;
use rusqlite::{Transaction, params};
use serde_json::Value;

pub(crate) fn migrate(tx: &Transaction<'_>) -> Result<()> {
    rewrite(
        tx,
        "SELECT rowid,'pane',payload FROM panes",
        "UPDATE panes SET payload=?1 WHERE rowid=?2",
    )?;
    rewrite(
        tx,
        "SELECT id,method,params FROM events WHERE method IN ('pane.created','pane.updated','pane.renamed','pane.state_changed')",
        "UPDATE events SET params=?1 WHERE id=?2",
    )?;
    rewrite(
        tx,
        "SELECT rowid,'receipt',result FROM idempotency",
        "UPDATE idempotency SET result=?1 WHERE rowid=?2",
    )?;
    tx.execute_batch(include_str!("../migrations/003-english-values.sql"))?;
    Ok(())
}

fn rewrite(tx: &Transaction<'_>, select: &str, update: &str) -> Result<()> {
    let mut statement = tx.prepare(select)?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (id, kind, raw) = row?;
        let mut value: Value = serde_json::from_str(&raw)?;
        let changed = match kind.as_str() {
            "pane" | "pane.state_changed" => translate_fields(&mut value),
            "pane.created" | "pane.updated" | "pane.renamed" | "receipt" => value
                .get_mut("pane")
                .filter(|pane| pane.get("paneId").is_some() && pane.get("workspaceId").is_some())
                .is_some_and(translate_fields),
            _ => false,
        };
        if changed {
            tx.execute(update, params![serde_json::to_string(&value)?, id])?;
        }
    }
    Ok(())
}

fn translate_fields(value: &mut Value) -> bool {
    let mut changed = false;
    if let Some(state) = value.get_mut("state") {
        let translated = match state.as_str() {
            Some("iniciando") => Some("starting"),
            Some("trabalhando") => Some("working"),
            Some("ociosa") => Some("idle"),
            Some("esperando_você") => Some("waiting_for_you"),
            Some("cancelando") => Some("cancelling"),
            Some("sem_sinal") => Some("no_signal"),
            Some("recuperando") => Some("recovering"),
            Some("morta") => Some("dead"),
            Some("não_sei") => Some("unknown"),
            _ => None,
        };
        if let Some(translated) = translated {
            *state = Value::String(translated.into());
            changed = true;
        }
    }
    if let Some(phrase) = value.get_mut("phrase") {
        let translated = match phrase.as_str() {
            Some("Iniciando shell") => Some("Starting shell"),
            Some("Restaurando shell após perda do processo") => {
                Some("Restoring shell after process loss")
            }
            Some("Shell encerrado") => Some("Shell exited"),
            Some("Shell ativo") => Some("Shell active"),
            _ => None,
        };
        if let Some(translated) = translated {
            *phrase = Value::String(translated.into());
            changed = true;
        }
    }
    changed
}
