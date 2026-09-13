use crate::database::{Change, Error, Result, Store};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use soyeht_protocol::EditorLanguage;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LanguageServerCheckpoint {
    pub workspace_id: String,
    pub language: EditorLanguage,
    pub payload: Value,
}

impl Store {
    pub fn language_server_checkpoints(&self) -> Result<Vec<LanguageServerCheckpoint>> {
        let mut statement = self.connection.prepare(
            "SELECT workspace_id,language,payload FROM language_server_checkpoints ORDER BY workspace_id,language",
        )?;
        statement
            .query_map([], |row| {
                let language: String = row.get(1)?;
                let payload: String = row.get(2)?;
                Ok((row.get::<_, String>(0)?, language, payload))
            })?
            .map(|row| {
                let (workspace_id, language, payload) = row?;
                Ok(LanguageServerCheckpoint {
                    workspace_id,
                    language: serde_json::from_value(Value::String(language))?,
                    payload: serde_json::from_str(&payload)?,
                })
            })
            .collect::<Result<_>>()
    }
}

impl Change<'_> {
    pub fn save_language_server_checkpoint(
        &self,
        checkpoint: &LanguageServerCheckpoint,
    ) -> Result<()> {
        validate(checkpoint)?;
        let language = serde_json::to_value(&checkpoint.language)?;
        let language = language
            .as_str()
            .ok_or(Error::InvalidLanguageServerCheckpoint)?;
        let payload = serde_json::to_string(&checkpoint.payload)?;
        self.connection().execute(
            "INSERT INTO language_server_checkpoints(workspace_id,language,payload) VALUES (?1,?2,?3) ON CONFLICT(workspace_id,language) DO UPDATE SET payload=excluded.payload",
            params![checkpoint.workspace_id, language, payload],
        )?;
        Ok(())
    }

    pub fn delete_language_server_checkpoint(
        &self,
        workspace_id: &str,
        language: &EditorLanguage,
    ) -> Result<()> {
        let language = serde_json::to_value(language)?;
        let language = language
            .as_str()
            .ok_or(Error::InvalidLanguageServerCheckpoint)?;
        self.connection().execute(
            "DELETE FROM language_server_checkpoints WHERE workspace_id=?1 AND language=?2",
            params![workspace_id, language],
        )?;
        Ok(())
    }
}

fn validate(checkpoint: &LanguageServerCheckpoint) -> Result<()> {
    let supported = matches!(
        checkpoint.language,
        EditorLanguage::Rust
            | EditorLanguage::Swift
            | EditorLanguage::Typescript
            | EditorLanguage::Python
    );
    let bytes = serde_json::to_vec(&checkpoint.payload)?;
    if checkpoint.workspace_id.is_empty()
        || checkpoint.workspace_id.len() > 128
        || !supported
        || !checkpoint.payload.is_object()
        || bytes.len() > soyeht_protocol::limits::JSON_BYTES
    {
        return Err(Error::InvalidLanguageServerCheckpoint);
    }
    Ok(())
}
