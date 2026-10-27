//! Replays a recorded engine state in the window, for comparing the desktop client with the
//! design and the Mac (`examples/design_audit.rs`). The state is a JSON file of the wire's own
//! shapes: workspaces, panes, the requests agents wait on, a transcript per agent pane (and
//! where its earlier page starts), the messages queued for an agent, what a terminal printed,
//! the usage readings, the harness catalogue with the models each lists, the settings and
//! skills Preferences show, and any further notifications to replay. Nothing is drawn that a
//! real engine could send, and no engine is needed.

use crate::{
    app::{self, Setup, TasksStart},
    destination::Destination,
    open_menu::Menu,
    prefs::Section,
    tasks_ui::TaskView,
    theme::ThemeName,
};
use serde_json::Value;
use soyeht_client::{
    Engine, Event, Snapshot,
    attachments::{Draft, Rejection, Source, Staged, decode_base64},
    catalogue::Catalogue,
    editor::EditorEvent,
    preferences::Preferences,
    streams::StreamOutput,
    tasks::TaskUpdate,
    timestamp,
};
use soyeht_protocol::{
    EditorTabsListResult, EventName, FileReadResult, ModelListResult, PromptAttachment,
};
use std::path::PathBuf;

/// A time in a fixture as `now-154` or `now+9000`, the moment that many seconds before or
/// after the window opens, so that a scene is the same each time it is drawn (a reset that
/// is written as a date goes by). The engine's timestamps replace them.
fn resolve_clock(value: &mut Value) {
    match value {
        Value::String(text) => {
            let offset = |sign: &str| {
                let seconds = text.strip_prefix(sign)?;
                let Ok(seconds) = seconds.parse::<i64>() else {
                    return None;
                };
                Some(seconds)
            };
            if let Some(ago) = offset("now-") {
                *text = timestamp::format(timestamp::now() as i64 - ago);
            } else if let Some(ahead) = offset("now+") {
                *text = timestamp::format(timestamp::now() as i64 + ahead);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(resolve_clock),
        Value::Object(fields) => fields.values_mut().for_each(resolve_clock),
        _ => {}
    }
}

/// The events a fixture replays, in the order an engine would send them.
pub fn events(fixture: &Value) -> Result<Vec<Event>, String> {
    let mut fixture = fixture.clone();
    resolve_clock(&mut fixture);
    let section = |name: &str| fixture.get(name).cloned().unwrap_or(Value::Null);
    let list = |name: &str| match section(name) {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    let snapshot = Snapshot {
        workspaces: serde_json::from_value(Value::Array(list("workspaces")))
            .map_err(describe("workspaces"))?,
        panes: serde_json::from_value(Value::Array(list("panes"))).map_err(describe("panes"))?,
        focus: match section("focus") {
            Value::Null => None,
            focus => Some(serde_json::from_value(focus).map_err(describe("focus"))?),
        },
        requests: list("requests"),
        loops: list("loops"),
        tasks: list("tasks"),
        attention: list("attention"),
        files: serde_json::from_value(Value::Array(list("files"))).map_err(describe("files"))?,
        writing: serde_json::from_value(Value::Array(list("writing")))
            .map_err(describe("writing"))?,
        cursor: 1,
    };
    let mut events = vec![Event::Connected(snapshot)];
    let cursors = section("olderCursors");
    if let Value::Object(transcripts) = section("transcripts") {
        for (pane_id, items) in transcripts {
            events.push(Event::Transcript {
                next_cursor: cursors
                    .get(&pane_id)
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                pane_id,
                items: serde_json::from_value(items).map_err(describe("transcripts"))?,
            });
        }
    }
    if let Value::Object(details) = section("taskDetails") {
        for (_, detail) in details {
            events.push(Event::Task(TaskUpdate::Detail(Box::new(
                serde_json::from_value(detail).map_err(describe("taskDetails"))?,
            ))));
        }
    }
    if let Value::Object(queues) = section("queues") {
        for (pane_id, queue) in queues {
            events.push(Event::Queue {
                pane_id,
                items: serde_json::from_value(queue.get("items").cloned().unwrap_or_default())
                    .map_err(describe("queues"))?,
                paused: queue
                    .get("paused")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
        }
    }
    if let Value::Object(screens) = section("terminals") {
        for (pane_id, text) in screens {
            let bytes = text.as_str().unwrap_or_default().as_bytes().to_vec();
            events.push(Event::Terminal(StreamOutput::Bytes { pane_id, bytes }));
        }
    }
    for pane_id in list("gaps").iter().filter_map(Value::as_str) {
        events.push(Event::Terminal(StreamOutput::Gap {
            pane_id: pane_id.to_owned(),
        }));
    }
    events.extend(editors(&fixture)?);
    events.push(Event::Usage(
        serde_json::from_value(Value::Array(list("usage"))).map_err(describe("usage"))?,
    ));
    events.push(Event::Harnesses(
        serde_json::from_value(Value::Array(list("harnesses"))).map_err(describe("harnesses"))?,
    ));
    events.extend(catalogues(&section("catalogues"))?);
    if let Value::Object(settings) = section("settings") {
        let mut preferences = Preferences::default();
        for (key, value) in &settings {
            if !preferences.apply(key, value) {
                return Err(format!(
                    "the fixture's settings name {key}, which is none of Preferences'"
                ));
            }
        }
        events.push(Event::Preferences(preferences));
    }
    if let Value::Object(commands) = section("commands") {
        for (pane_id, items) in commands {
            events.push(Event::PaneCommands {
                pane_id,
                items: serde_json::from_value(items).map_err(describe("commands"))?,
            });
        }
    }
    if !section("skills").is_null() {
        events.push(Event::Skills {
            workspace_id: None,
            listing: serde_json::from_value(section("skills")).map_err(describe("skills"))?,
        });
    }
    for notification in list("notifications") {
        let name = serde_json::from_value::<EventName>(notification["name"].clone())
            .map_err(describe("notifications"))?;
        events.push(Event::Notification {
            name,
            params: notification["params"].clone(),
        });
    }
    Ok(events)
}

/// What the engine told the editors: the page of each open file, the tabs of each workspace and
/// the folders of the project trees.
fn editors(fixture: &Value) -> Result<Vec<Event>, String> {
    let mut events = Vec::new();
    if let Some(Value::Object(pages)) = fixture.get("pages") {
        for page in pages.values() {
            let page: FileReadResult =
                serde_json::from_value(page.clone()).map_err(describe("pages"))?;
            events.push(Event::Editor(EditorEvent::Page(Box::new(page))));
        }
    }
    if let Some(Value::Object(tabs)) = fixture.get("tabs") {
        for (workspace_id, tabs) in tabs {
            events.push(Event::Editor(EditorEvent::Tabs(EditorTabsListResult {
                workspace_id: workspace_id.clone(),
                tabs: serde_json::from_value(tabs.clone()).map_err(describe("tabs"))?,
            })));
        }
    }
    if let Some(Value::Array(listings)) = fixture.get("trees") {
        for listing in listings {
            let listing = serde_json::from_value(listing.clone()).map_err(describe("trees"))?;
            events.push(Event::Editor(EditorEvent::Listed(Box::new(listing))));
        }
    }
    Ok(events)
}

/// The models each harness lists, as the engine's `model.list` answers.
fn catalogues(section: &Value) -> Result<Vec<Event>, String> {
    let Value::Object(harnesses) = section else {
        return Ok(Vec::new());
    };
    harnesses
        .iter()
        .map(|(harness, listing)| {
            let page: ModelListResult =
                serde_json::from_value(listing.clone()).map_err(describe("catalogues"))?;
            Ok(Event::Catalogue {
                harness: harness.clone(),
                catalogue: Catalogue {
                    models: page.items,
                    options: page.options,
                    source: page.source,
                },
            })
        })
        .collect()
}

/// The files the fixture says are attached in each composer (and not yet sent), and the refusal
/// each composer last showed. A file is drawn as a chip; its bytes are never read.
pub fn staged(fixture: &Value) -> Result<Vec<(String, Staged)>, String> {
    let Some(Value::Object(panes)) = fixture.get("staged") else {
        return Ok(Vec::new());
    };
    panes
        .iter()
        .map(|(pane_id, files)| {
            let files: Vec<PromptAttachment> =
                serde_json::from_value(files.clone()).map_err(describe("staged"))?;
            let mut results: Vec<Result<Draft, Rejection>> = files
                .into_iter()
                .map(|file| {
                    let size = file.bytes.parse().unwrap_or(0);
                    let thumbnail = file.thumbnail.as_deref().and_then(decode_base64);
                    Ok(Draft::new(
                        file.kind,
                        file.name,
                        file.mime_type,
                        size,
                        Source::HostFile(file.path.unwrap_or_default()),
                        thumbnail,
                    ))
                })
                .collect();
            if let Some(refused) = fixture.get("refused").and_then(|all| all.get(pane_id)) {
                let name = refused["name"].as_str().unwrap_or_default().to_owned();
                results.push(Err(match refused["rejection"].as_str() {
                    Some("tooMany") => Rejection::TooMany,
                    Some("totalTooLarge") => Rejection::TotalTooLarge,
                    Some("empty") => Rejection::Empty { name },
                    Some("unreadable") => Rejection::Unreadable { name },
                    Some("folder") => Rejection::Folder { name },
                    _ => Rejection::TooLarge { name },
                }));
            }
            let mut staged = Staged::default();
            staged.add(results);
            Ok((pane_id.clone(), staged))
        })
        .collect()
}

/// What the fixture says is written in each composer and not yet sent.
pub fn drafts(fixture: &Value) -> Vec<(String, String)> {
    let Some(Value::Object(panes)) = fixture.get("drafts") else {
        return Vec::new();
    };
    panes
        .iter()
        .filter_map(|(pane_id, text)| Some((pane_id.clone(), text.as_str()?.to_owned())))
        .collect()
}

fn describe(section: &str) -> impl Fn(serde_json::Error) -> String + '_ {
    move |error| format!("the fixture's {section} are not the wire's shapes: {error}")
}

/// The menu a spec names: `new-pane`, `overflow`, `settings` or `settings:<panel>`,
/// `harness:<pane>`, `loop:<pane>` or `config:<pane>:<option>`.
pub fn menu(spec: &str) -> Option<Menu> {
    let mut parts = spec.splitn(3, ':');
    match (parts.next()?, parts.next(), parts.next()) {
        ("new-pane", None, None) => Some(Menu::NewPane),
        ("overflow", None, None) => Some(Menu::Overflow),
        ("settings", None, None) => Some(Menu::Settings),
        ("settings", Some(panel), None) => section(panel).map(|_| Menu::Settings),
        ("harness", Some(pane), None) => Some(Menu::Harness(pane.to_owned())),
        ("loop", Some(pane), None) => Some(Menu::Loop(pane.to_owned())),
        ("config", Some(pane), Some(config)) => Some(Menu::Config {
            pane_id: pane.to_owned(),
            config_id: config.to_owned(),
        }),
        _ => None,
    }
}

/// The Preferences panel a spec names (`settings:usage`), when it names one.
pub fn panel(spec: &str) -> Option<Section> {
    section(spec.strip_prefix("settings:")?)
}

/// The panel called `name` (`appearance`, `harnesses`, `usage`, `skills`).
fn section(name: &str) -> Option<Section> {
    Section::ALL
        .into_iter()
        .find(|section| section.title().eq_ignore_ascii_case(name))
}

/// The place of the rail a spec names (`editor`, `terminal`), when it names one that draws panes.
pub fn place(spec: &str) -> Option<Destination> {
    Destination::ALL
        .into_iter()
        .find(|place| place.is_pane_grid() && place.label().eq_ignore_ascii_case(spec))
}

/// The tasks surface a spec names: `tasks` (the board), `tasks:list`, `task:<id>` (the board with
/// that task's detail) or `new-task` (the board with the new-task form).
pub fn tasks_start(spec: &str) -> Option<TasksStart> {
    let (view, task, new_task) = match spec.split_once(':') {
        None if spec == "tasks" => (TaskView::Board, None, false),
        None if spec == "new-task" => (TaskView::Board, None, true),
        Some(("tasks", "list")) => (TaskView::List, None, false),
        Some(("task", id)) => (TaskView::Board, Some(id.to_owned()), false),
        _ => return None,
    };
    Some(TasksStart {
        view,
        task,
        new_task,
    })
}

/// Opens the window on `fixture`, in `theme`, with the menu `open` names open (see [`menu`]) or the
/// place of the rail it names (see [`place`]),
/// in a window of `size` points when given, and runs until it closes.
pub fn run(
    fixture: &Value,
    theme: ThemeName,
    open: Option<&str>,
    size: Option<(f32, f32)>,
    notice: app::Notice,
) -> Result<(), String> {
    let events = events(fixture)?;
    let staged = staged(fixture)?;
    let drafts = drafts(fixture);
    let tasks = open.and_then(tasks_start);
    let panel = open.and_then(panel);
    let destination = open.and_then(place);
    let menu = match (open, &tasks, destination) {
        (Some(spec), None, None) => {
            Some(menu(spec).ok_or_else(|| format!("no menu is named {spec}"))?)
        }
        _ => None,
    };
    app::launch(notice, move || {
        // The engine the window holds has nothing to reach: the window draws the replay only.
        let (engine, unreached) = Engine::start(PathBuf::from("no-engine.sock"));
        // Its threads stop when nobody listens, and then every command would fail: the window
        // keeps the receiver for as long as the process lives.
        std::mem::forget(unreached);
        let (sender, inbox) = async_channel::unbounded();
        for event in events {
            if sender.try_send(event).is_err() {
                break;
            }
        }
        Setup {
            engine,
            inbox,
            theme,
            preferences: None,
            menu,
            panel,
            size,
            staged,
            drafts,
            tasks,
            destination,
            web: None,
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_panel_is_found_by_its_name() {
        assert_eq!(panel("settings:usage"), Some(Section::Usage));
        assert_eq!(panel("settings:Skills"), Some(Section::Skills));
        assert_eq!(panel("settings:shortcuts"), None);
        assert_eq!(panel("usage"), None);
    }

    #[test]
    fn a_fixture_replays_as_a_connection_then_transcripts_screens_gaps_usage_and_harnesses() {
        let fixture = json!({
            "workspaces": [{"workspaceId": "w", "name": "w", "path": "/", "layout": {}}],
            "panes": [],
            "transcripts": {"a": []},
            "terminals": {"t": "hi"},
            "gaps": ["t"],
        });
        let kinds: Vec<_> = events(&fixture)
            .unwrap()
            .iter()
            .map(|event| match event {
                Event::Connected(_) => "connected",
                Event::Transcript { .. } => "transcript",
                Event::Terminal(StreamOutput::Bytes { .. }) => "terminal",
                Event::Terminal(_) => "gap",
                Event::Usage(_) => "usage",
                Event::Harnesses(_) => "harnesses",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "connected",
                "transcript",
                "terminal",
                "gap",
                "usage",
                "harnesses"
            ]
        );
    }

    #[test]
    fn the_design_audit_fixture_is_made_of_the_wires_shapes() {
        let text = include_str!("../examples/design_audit.json");
        let events = events(&serde_json::from_str(text).unwrap()).unwrap();
        assert!(events.len() > 5);
    }

    #[test]
    fn a_time_written_as_so_long_ago_becomes_a_timestamp() {
        let fixture = json!({"panes": [], "workspaces": [], "queues": {"p": {"items": [{
            "promptId": "q", "text": "t", "origin": "human", "status": "queued", "queuedAt": "now-60"
        }]}}});
        let events = events(&fixture).unwrap();
        let Some(Event::Queue { items, .. }) =
            events.iter().find(|e| matches!(e, Event::Queue { .. }))
        else {
            panic!("no queue");
        };
        let queued = timestamp::seconds_since_epoch(&items[0].queued_at).unwrap();
        assert!((timestamp::now() - queued - 60.0).abs() < 5.0);
    }

    #[test]
    fn a_time_written_as_so_long_from_now_becomes_a_timestamp() {
        let fixture = json!({"panes": [], "workspaces": [], "usage": [{
            "provider": "claude", "state": "ok", "source": "claude_oauth_endpoint",
            "fetchedAt": "now-60",
            "primary": {"used": 0.4, "resetsAt": "now+9000", "label": "Session (5 h)"}
        }]});
        let events = events(&fixture).unwrap();
        let Some(Event::Usage(readings)) = events.iter().find(|e| matches!(e, Event::Usage(_)))
        else {
            panic!("no usage");
        };
        let resets = readings[0]
            .primary
            .as_ref()
            .map(|window| window.resets_at.clone());
        let resets = timestamp::seconds_since_epoch(&resets.unwrap()).unwrap();
        assert!((resets - timestamp::now() - 9000.0).abs() < 5.0);
    }

    #[test]
    fn the_features_fixture_is_made_of_the_wires_shapes() {
        let text = include_str!("../examples/design_audit_features.json");
        assert!(events(&serde_json::from_str(text).unwrap()).unwrap().len() > 5);
    }

    #[test]
    fn a_section_that_is_not_the_wires_shape_is_refused_by_name() {
        let error = events(&json!({"panes": [{"paneId": 1}]})).unwrap_err();
        assert!(error.contains("panes"), "{error}");
    }

    #[test]
    fn menus_are_named_by_a_short_spec() {
        assert_eq!(menu("new-pane"), Some(Menu::NewPane));
        assert_eq!(menu("harness:p1"), Some(Menu::Harness("p1".into())));
        assert_eq!(
            menu("config:p1:model"),
            Some(Menu::Config {
                pane_id: "p1".into(),
                config_id: "model".into()
            })
        );
        assert_eq!(menu("settings:usage"), Some(Menu::Settings));
        assert_eq!(panel("settings:usage"), Some(Section::Usage));
        assert_eq!(menu("settings:shortcuts"), None);
        assert_eq!(panel("settings"), None);
        assert_eq!(menu("nonsense"), None);
    }
}
