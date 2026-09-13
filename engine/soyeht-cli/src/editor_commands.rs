use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use soyeht_cli::{Client, Error, Result};
use soyeht_protocol::*;
use std::collections::BTreeMap;

const DEFAULT_VIEWPORT_LINES: u32 = 200;
const DEFAULT_PAGE: u32 = 100;

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidArguments(message.into())
}

fn rpc<T: DeserializeOwned + Serialize>(
    client: &mut Client,
    method: &str,
    params: Value,
) -> Result<Value> {
    Ok(serde_json::to_value(client.request::<T>(method, params)?)?)
}

struct Flags {
    values: BTreeMap<String, String>,
}

impl Flags {
    fn parse(args: &[&str], allowed: &[&str]) -> Result<Self> {
        let mut values = BTreeMap::new();
        let mut index = 0;
        while index < args.len() {
            let name = args[index];
            if !allowed.contains(&name) {
                return Err(invalid(format!("Unknown option: {name}")));
            }
            let value = args
                .get(index + 1)
                .ok_or_else(|| invalid(format!("Missing value for {name}")))?;
            if values.insert(name.into(), (*value).into()).is_some() {
                return Err(invalid(format!("Repeated option: {name}")));
            }
            index += 2;
        }
        Ok(Self { values })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    fn number(&self, name: &str, default: u32) -> Result<u32> {
        self.get(name)
            .map_or(Ok(default), |value| number(value, name))
    }

    fn boolean(&self, name: &str, default: bool) -> Result<bool> {
        self.get(name).map_or(Ok(default), |value| match value {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(invalid(format!("{name} must be true or false"))),
        })
    }
}

enum Request {
    Open(Value),
    Read(Value),
    Write(Value),
    History(Value),
    Revert(Value),
    Search(Value),
    Symbols(Value),
    Diff(Value),
    UsageList(Value),
    UsageRefresh(Value),
}

impl Request {
    fn run(self, client: &mut Client) -> Result<Value> {
        match self {
            Self::Open(params) => rpc::<FileOpenResult>(client, "file.open", params),
            Self::Read(params) => rpc::<FileReadResult>(client, "file.read", params),
            Self::Write(params) => rpc::<FileWriteResult>(client, "file.write", params),
            Self::History(params) => rpc::<FileHistoryResult>(client, "file.history", params),
            Self::Revert(params) => rpc::<FileRevertResult>(client, "file.revert", params),
            Self::Search(params) => rpc::<FileSearchResult>(client, "file.search", params),
            Self::Symbols(params) => rpc::<FileSymbolsResult>(client, "file.symbols", params),
            Self::Diff(params) => rpc::<FileDiffResult>(client, "file.diff", params),
            Self::UsageList(params) => rpc::<UsageListResult>(client, "usage.list", params),
            Self::UsageRefresh(params) => {
                rpc::<UsageRefreshResult>(client, "usage.refresh", params)
            }
        }
    }
}

pub fn execute_file(client: &mut Client, args: &[&str], key: &str) -> Result<Value> {
    parse_file(args, key)?.run(client)
}

pub fn execute_usage(client: &mut Client, args: &[&str], key: &str) -> Result<Value> {
    parse_usage(args, key)?.run(client)
}

fn parse_file(args: &[&str], key: &str) -> Result<Request> {
    match args {
        ["open", workspace, path, rest @ ..] => open(workspace, path, rest, key),
        ["read", file, rest @ ..] => read(file, rest),
        [
            "write",
            file,
            revision,
            start_line,
            start_column,
            end_line,
            end_column,
            text,
        ] => write(
            file,
            revision,
            [start_line, start_column, end_line, end_column],
            text,
            key,
        ),
        ["history", file, rest @ ..] => history(file, rest),
        ["revert", file, version, revision] => revert(file, version, revision, key),
        ["search", workspace, query, rest @ ..] => search(workspace, query, rest, key),
        ["symbols", file, revision] => symbols(file, revision),
        ["diff", file, revision] => diff(file, revision),
        _ => Err(invalid(crate::commands::USAGE)),
    }
}

fn parse_usage(args: &[&str], key: &str) -> Result<Request> {
    match args {
        ["list"] => Ok(Request::UsageList(json!({}))),
        ["refresh"] => Ok(Request::UsageRefresh(json!({"idempotencyKey":key}))),
        ["refresh", provider] if matches!(*provider, "claude" | "codex" | "cursor") => Ok(
            Request::UsageRefresh(json!({"provider":provider,"idempotencyKey":key})),
        ),
        ["refresh", _] => Err(invalid("Provider must be claude, codex or cursor")),
        _ => Err(invalid(crate::commands::USAGE)),
    }
}

fn open(workspace: &str, path: &str, rest: &[&str], key: &str) -> Result<Request> {
    let flags = Flags::parse(rest, &["--line", "--column"])?;
    let mut params = Map::new();
    insert(&mut params, "workspaceId", workspace);
    insert(&mut params, "path", path);
    optional_number(&mut params, "line", flags.get("--line"))?;
    optional_number(&mut params, "column", flags.get("--column"))?;
    insert(&mut params, "idempotencyKey", key);
    Ok(Request::Open(params.into()))
}

fn read(file: &str, rest: &[&str]) -> Result<Request> {
    let flags = Flags::parse(
        rest,
        &["--revision", "--source", "--version", "--from", "--lines"],
    )?;
    if let Some(source) = flags.get("--source")
        && !matches!(source, "buffer" | "disk" | "draft" | "history")
    {
        return Err(invalid("Source must be buffer, disk, draft or history"));
    }
    let mut params = Map::new();
    insert(&mut params, "fileId", file);
    optional(&mut params, "revision", flags.get("--revision"));
    optional(&mut params, "source", flags.get("--source"));
    optional(&mut params, "versionId", flags.get("--version"));
    params.insert("fromLine".into(), json!(flags.number("--from", 0)?));
    params.insert(
        "lineCount".into(),
        json!(flags.number("--lines", DEFAULT_VIEWPORT_LINES)?),
    );
    Ok(Request::Read(params.into()))
}

fn write(
    file: &str,
    revision: &str,
    positions: [&str; 4],
    text: &str,
    key: &str,
) -> Result<Request> {
    let start_line = number(positions[0], "start line")?;
    let start_column = number(positions[1], "start column")?;
    let end_line = number(positions[2], "end line")?;
    let end_column = number(positions[3], "end column")?;
    Ok(Request::Write(json!({
        "fileId":file,"baseRevision":revision,
        "edits":[{"range":{"start":{"line":start_line,"column":start_column},"end":{"line":end_line,"column":end_column}},"text":text}],
        "author":{"actor":{"role":"human_local","client":"soyeht-cli"},"name":"you"},
        "idempotencyKey":key
    })))
}

fn history(file: &str, rest: &[&str]) -> Result<Request> {
    let flags = Flags::parse(rest, &["--cursor", "--limit"])?;
    let mut params = Map::new();
    insert(&mut params, "fileId", file);
    optional(&mut params, "cursor", flags.get("--cursor"));
    params.insert(
        "limit".into(),
        json!(flags.number("--limit", DEFAULT_PAGE)?),
    );
    Ok(Request::History(params.into()))
}

fn revert(file: &str, version: &str, revision: &str, key: &str) -> Result<Request> {
    Ok(Request::Revert(json!({
        "fileId":file,"versionId":version,"baseRevision":revision,"idempotencyKey":key
    })))
}

fn search(workspace: &str, query: &str, rest: &[&str], key: &str) -> Result<Request> {
    let flags = Flags::parse(
        rest,
        &["--scope", "--replacement", "--case-sensitive", "--limit"],
    )?;
    if let Some(scope) = flags.get("--scope")
        && !matches!(scope, "content" | "path")
    {
        return Err(invalid("Scope must be content or path"));
    }
    let mut params = Map::new();
    insert(&mut params, "workspaceId", workspace);
    insert(&mut params, "query", query);
    optional(&mut params, "scope", flags.get("--scope"));
    optional(&mut params, "replacement", flags.get("--replacement"));
    params.insert(
        "caseSensitive".into(),
        json!(flags.boolean("--case-sensitive", false)?),
    );
    params.insert(
        "limit".into(),
        json!(flags.number("--limit", DEFAULT_PAGE)?),
    );
    insert(&mut params, "idempotencyKey", key);
    Ok(Request::Search(params.into()))
}

fn symbols(file: &str, revision: &str) -> Result<Request> {
    Ok(Request::Symbols(json!({"fileId":file,"revision":revision})))
}

fn diff(file: &str, revision: &str) -> Result<Request> {
    Ok(Request::Diff(json!({"fileId":file,"revision":revision})))
}

fn optional_number(params: &mut Map<String, Value>, name: &str, value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        params.insert(name.into(), json!(number(value, name)?));
    }
    Ok(())
}

fn optional(params: &mut Map<String, Value>, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        insert(params, name, value);
    }
}

fn insert(params: &mut Map<String, Value>, name: &str, value: &str) {
    params.insert(name.into(), json!(value));
}

fn number(value: &str, name: &str) -> Result<u32> {
    value
        .parse()
        .map_err(|_| invalid(format!("{name} must be an integer")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(request: Request) -> Value {
        match request {
            Request::Open(value)
            | Request::Read(value)
            | Request::Write(value)
            | Request::History(value)
            | Request::Revert(value)
            | Request::Search(value)
            | Request::Symbols(value)
            | Request::Diff(value)
            | Request::UsageList(value)
            | Request::UsageRefresh(value) => value,
        }
    }

    #[test]
    fn every_editor_and_usage_command_builds_bounded_typed_parameters() {
        let open =
            value(parse_file(&["open", "workspace", "src/main.rs", "--line", "4"], "k").unwrap());
        assert_eq!(open["line"], 4);
        assert_eq!(
            value(parse_file(&["read", "file"], "k").unwrap())["lineCount"],
            200
        );
        let write =
            value(parse_file(&["write", "file", "7", "1", "2", "3", "4", "safe"], "k").unwrap());
        assert_eq!(write["edits"][0]["range"]["end"]["column"], 4);
        assert_eq!(
            value(parse_file(&["history", "file"], "k").unwrap())["limit"],
            100
        );
        assert_eq!(
            value(parse_file(&["revert", "file", "version", "8"], "k").unwrap())["versionId"],
            "version"
        );
        assert_eq!(
            value(parse_file(&["search", "workspace", "needle", "--scope", "path"], "k",).unwrap())
                ["scope"],
            "path"
        );
        assert_eq!(
            value(parse_file(&["symbols", "file", "9"], "k").unwrap())["revision"],
            "9"
        );
        assert_eq!(
            value(parse_file(&["diff", "file", "9"], "k").unwrap())["fileId"],
            "file"
        );
        assert!(
            value(parse_usage(&["list"], "k").unwrap())
                .as_object()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            value(parse_usage(&["refresh", "cursor"], "k").unwrap())["provider"],
            "cursor"
        );
    }
}
