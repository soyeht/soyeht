//! Named persistence operations for browser session grants.

use crate::database::{Change, Error, Result, Store};
use rusqlite::{OptionalExtension, params};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebSessionGrantScope {
    Task(String),
    Workspace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebSessionGrant {
    pub workspace_id: String,
    pub domain: String,
    pub scope: WebSessionGrantScope,
    pub granted_by: String,
    pub created_ms: i64,
}

impl Store {
    pub fn web_session_grant(
        &self,
        workspace_id: &str,
        domain: &str,
        task_id: Option<&str>,
    ) -> Result<Option<WebSessionGrant>> {
        validate_lookup(workspace_id, domain, task_id)?;
        let row = self
            .connection
            .query_row(
                "SELECT scope,task_id,granted_by,created_ms FROM web_session_grants WHERE workspace_id=?1 AND domain=?2 AND (scope='workspace' OR (scope='task' AND task_id=?3)) ORDER BY CASE scope WHEN 'task' THEN 0 ELSE 1 END LIMIT 1",
                params![workspace_id, domain, task_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(scope, task_id, granted_by, created_ms)| {
            let scope = decode_scope(&scope, task_id)?;
            Ok(WebSessionGrant {
                workspace_id: workspace_id.to_owned(),
                domain: domain.to_owned(),
                scope,
                granted_by,
                created_ms,
            })
        })
        .transpose()
    }

    pub fn web_session_grants(&self, workspace_id: &str) -> Result<Vec<WebSessionGrant>> {
        validate_id(workspace_id)?;
        let mut statement = self.connection.prepare(
            "SELECT domain,scope,task_id,granted_by,created_ms FROM web_session_grants WHERE workspace_id=?1 ORDER BY domain,scope,task_id",
        )?;
        let rows = statement.query_map([workspace_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut grants = Vec::new();
        for row in rows {
            let (domain, scope, task_id, granted_by, created_ms) = row?;
            let scope = decode_scope(&scope, task_id)?;
            let grant = WebSessionGrant {
                workspace_id: workspace_id.to_owned(),
                domain,
                scope,
                granted_by,
                created_ms,
            };
            validate_grant(&grant)?;
            grants.push(grant);
        }
        Ok(grants)
    }
}

impl Change<'_> {
    pub fn save_web_session_grant(&self, grant: &WebSessionGrant) -> Result<()> {
        validate_grant(grant)?;
        let (scope, task_id) = encode_scope(&grant.scope);
        self.transaction.execute(
            "INSERT INTO web_session_grants(workspace_id,domain,scope,task_id,granted_by,created_ms) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(workspace_id,domain,scope,ifnull(task_id,'')) DO UPDATE SET granted_by=excluded.granted_by,created_ms=excluded.created_ms",
            params![
                grant.workspace_id,
                grant.domain,
                scope,
                task_id,
                grant.granted_by,
                grant.created_ms,
            ],
        )?;
        Ok(())
    }

    pub fn remove_web_session_grant(
        &self,
        workspace_id: &str,
        domain: &str,
        scope: &WebSessionGrantScope,
    ) -> Result<bool> {
        validate_lookup(
            workspace_id,
            domain,
            match scope {
                WebSessionGrantScope::Task(task_id) => Some(task_id),
                WebSessionGrantScope::Workspace => None,
            },
        )?;
        let (scope, task_id) = encode_scope(scope);
        Ok(self.transaction.execute(
            "DELETE FROM web_session_grants WHERE workspace_id=?1 AND domain=?2 AND scope=?3 AND task_id IS ?4",
            params![workspace_id, domain, scope, task_id],
        )? == 1)
    }
}

fn encode_scope(scope: &WebSessionGrantScope) -> (&'static str, Option<&str>) {
    match scope {
        WebSessionGrantScope::Task(task_id) => ("task", Some(task_id)),
        WebSessionGrantScope::Workspace => ("workspace", None),
    }
}

fn decode_scope(scope: &str, task_id: Option<String>) -> Result<WebSessionGrantScope> {
    match (scope, task_id) {
        ("task", Some(task_id)) if validate_id(&task_id).is_ok() => {
            Ok(WebSessionGrantScope::Task(task_id))
        }
        ("workspace", None) => Ok(WebSessionGrantScope::Workspace),
        _ => Err(Error::InvalidWebSessionGrant),
    }
}

fn validate_lookup(workspace_id: &str, domain: &str, task_id: Option<&str>) -> Result<()> {
    validate_id(workspace_id)?;
    validate_domain(domain)?;
    if let Some(task_id) = task_id {
        validate_id(task_id)?;
    }
    Ok(())
}

fn validate_grant(grant: &WebSessionGrant) -> Result<()> {
    let task_id = match &grant.scope {
        WebSessionGrantScope::Task(task_id) => Some(task_id.as_str()),
        WebSessionGrantScope::Workspace => None,
    };
    validate_lookup(&grant.workspace_id, &grant.domain, task_id)?;
    if grant.granted_by.is_empty() || grant.granted_by.len() > 128 || grant.created_ms < 0 {
        return Err(Error::InvalidWebSessionGrant);
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 {
        return Err(Error::InvalidWebSessionGrant);
    }
    Ok(())
}

fn validate_domain(domain: &str) -> Result<()> {
    if domain.is_empty()
        || domain.len() > 255
        || domain != domain.to_ascii_lowercase()
        || domain.bytes().any(|byte| {
            !byte.is_ascii_alphanumeric() && !matches!(byte, b'.' | b'-' | b':' | b'[' | b']')
        })
    {
        return Err(Error::InvalidWebSessionGrant);
    }
    Ok(())
}
