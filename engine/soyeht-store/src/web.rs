//! Named persistence operations for browser session grants.

use crate::database::{Change, Error, Result, Store};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use soyeht_protocol::{
    PanePermissionAnswerResult, PermissionRequest, PermissionRequestKind, limits,
};

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

#[derive(Clone, Debug)]
pub struct WebPermissionRequest {
    pub request_id: String,
    pub pane_id: String,
    pub workspace_id: String,
    pub generation: u32,
    pub task_id: Option<String>,
    pub kind: PermissionRequestKind,
    pub domain: String,
    pub public: PermissionRequest,
    pub expires_ms: i64,
    pub result: Option<PanePermissionAnswerResult>,
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

    pub fn web_permission_request(&self, request_id: &str) -> Result<Option<WebPermissionRequest>> {
        validate_id(request_id)?;
        self.connection
            .query_row(
                "SELECT request_id,pane_id,workspace_id,generation,task_id,kind,domain,public,expires_ms,result FROM web_permission_requests WHERE request_id=?1",
                [request_id],
                permission_row,
            )
            .optional()?
            .map(valid_permission)
            .transpose()
    }

    pub fn open_web_permission(
        &self,
        pane_id: &str,
        kind: &PermissionRequestKind,
        domain: &str,
        task_id: Option<&str>,
    ) -> Result<Option<WebPermissionRequest>> {
        validate_id(pane_id)?;
        validate_domain(domain)?;
        if let Some(task_id) = task_id {
            validate_id(task_id)?;
        }
        let kind = encode_permission_kind(kind)?;
        self.connection
            .query_row(
                "SELECT request_id,pane_id,workspace_id,generation,task_id,kind,domain,public,expires_ms,result FROM web_permission_requests WHERE pane_id=?1 AND kind=?2 AND domain=?3 AND task_id IS ?4 AND result IS NULL",
                params![pane_id, kind, domain, task_id],
                permission_row,
            )
            .optional()?
            .map(valid_permission)
            .transpose()
    }

    pub fn web_permission_for_operation(
        &self,
        pane_id: &str,
        kind: &PermissionRequestKind,
        operation_key: &str,
    ) -> Result<Option<WebPermissionRequest>> {
        validate_id(pane_id).map_err(|_| Error::InvalidWebPermission)?;
        validate_id(operation_key).map_err(|_| Error::InvalidWebPermission)?;
        let kind = encode_permission_kind(kind)?;
        self.connection
            .query_row(
                "SELECT request_id,pane_id,workspace_id,generation,task_id,kind,domain,public,expires_ms,result FROM web_permission_requests WHERE pane_id=?1 AND kind=?2 AND json_extract(public,'$.promptId')=?3",
                params![pane_id, kind, operation_key],
                permission_row,
            )
            .optional()?
            .map(valid_permission)
            .transpose()
    }

    pub fn expired_web_permissions(&self, now_ms: i64) -> Result<Vec<WebPermissionRequest>> {
        if now_ms < 0 {
            return Err(Error::InvalidWebPermission);
        }
        self.rested_rows(
            "web_permissions",
            now_ms,
            || {
                let mut statement = self.connection.prepare_cached(
                    "SELECT request_id,pane_id,workspace_id,generation,task_id,kind,domain,public,expires_ms,result FROM web_permission_requests WHERE result IS NULL AND expires_ms<=?1 ORDER BY expires_ms,request_id",
                )?;
                statement
                    .query_map([now_ms], permission_row)?
                    .map(|row| {
                        row.and_then(|request| valid_permission(request).map_err(to_sql_error))
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(Into::into)
            },
            Store::next_web_permission_expiry_ms,
        )
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

    pub fn save_web_permission_request(&self, request: &WebPermissionRequest) -> Result<()> {
        valid_permission(request.clone())?;
        if request.result.is_some() {
            return Err(Error::InvalidWebPermission);
        }
        let pane_matches: bool = self.transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM panes WHERE id=?1 AND workspace_id=?2 AND closed=0)",
            params![request.pane_id, request.workspace_id],
            |row| row.get(0),
        )?;
        if !pane_matches {
            return Err(Error::InvalidWebPermission);
        }
        self.transaction.execute(
            "INSERT INTO web_permission_requests(request_id,pane_id,workspace_id,generation,task_id,kind,domain,public,expires_ms,result) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)",
            params![
                request.request_id,
                request.pane_id,
                request.workspace_id,
                request.generation,
                request.task_id,
                encode_permission_kind(&request.kind)?,
                request.domain,
                serde_json::to_string(&request.public)?,
                request.expires_ms,
            ],
        )?;
        Ok(())
    }

    pub fn answer_web_permission(
        &self,
        request_id: &str,
        result: &PanePermissionAnswerResult,
    ) -> Result<bool> {
        validate_id(request_id)?;
        if result.request_id != request_id {
            return Err(Error::InvalidWebPermission);
        }
        let raw = serde_json::to_string(result)?;
        if raw.len() > 4096 {
            return Err(Error::InvalidWebPermission);
        }
        Ok(self.transaction.execute(
            "UPDATE web_permission_requests SET result=?1 WHERE request_id=?2 AND result IS NULL",
            params![raw, request_id],
        )? == 1)
    }
}

pub(crate) fn permission_snapshot(connection: &Connection) -> Result<Vec<Value>> {
    let mut statement = connection.prepare(
        "SELECT public FROM web_permission_requests WHERE result IS NULL ORDER BY request_id",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.map(|raw| {
        let request: PermissionRequest = serde_json::from_str(&raw?)?;
        Ok(json!({"kind":"permission","request":request}))
    })
    .collect()
}

fn permission_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WebPermissionRequest> {
    let kind: String = row.get(5)?;
    let public: String = row.get(7)?;
    let result: Option<String> = row.get(9)?;
    Ok(WebPermissionRequest {
        request_id: row.get(0)?,
        pane_id: row.get(1)?,
        workspace_id: row.get(2)?,
        generation: row.get(3)?,
        task_id: row.get(4)?,
        kind: decode_permission_kind(&kind).map_err(to_sql_error)?,
        domain: row.get(6)?,
        public: serde_json::from_str(&public).map_err(to_sql_error)?,
        expires_ms: row.get(8)?,
        result: result
            .map(|value| serde_json::from_str(&value).map_err(to_sql_error))
            .transpose()?,
    })
}

fn to_sql_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn encode_permission_kind(kind: &PermissionRequestKind) -> Result<&'static str> {
    match kind {
        PermissionRequestKind::WebSession => Ok("web_session"),
        PermissionRequestKind::WebEvaluate => Ok("web_evaluate"),
        PermissionRequestKind::Agent => Err(Error::InvalidWebPermission),
    }
}

fn decode_permission_kind(kind: &str) -> Result<PermissionRequestKind> {
    match kind {
        "web_session" => Ok(PermissionRequestKind::WebSession),
        "web_evaluate" => Ok(PermissionRequestKind::WebEvaluate),
        _ => Err(Error::InvalidWebPermission),
    }
}

fn valid_permission(request: WebPermissionRequest) -> Result<WebPermissionRequest> {
    validate_id(&request.request_id).map_err(|_| Error::InvalidWebPermission)?;
    validate_id(&request.pane_id).map_err(|_| Error::InvalidWebPermission)?;
    validate_id(&request.workspace_id).map_err(|_| Error::InvalidWebPermission)?;
    validate_domain(&request.domain).map_err(|_| Error::InvalidWebPermission)?;
    if request.generation == 0 || request.expires_ms < 0 {
        return Err(Error::InvalidWebPermission);
    }
    if let Some(task_id) = &request.task_id {
        validate_id(task_id).map_err(|_| Error::InvalidWebPermission)?;
    }
    let public = &request.public;
    let identity_matches = public.request_id == request.request_id
        && public.pane_id == request.pane_id
        && public.generation == request.generation
        && public.kind.as_ref() == Some(&request.kind)
        && public.domain.as_deref() == Some(&request.domain)
        && !public.prompt_id.is_empty()
        && !public.options.is_empty();
    let public_bytes = serde_json::to_vec(public)?.len();
    if !identity_matches || public_bytes > limits::JSON_BYTES {
        return Err(Error::InvalidWebPermission);
    }
    if request
        .result
        .as_ref()
        .is_some_and(|result| result.request_id != request.request_id)
    {
        return Err(Error::InvalidWebPermission);
    }
    encode_permission_kind(&request.kind)?;
    Ok(request)
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
