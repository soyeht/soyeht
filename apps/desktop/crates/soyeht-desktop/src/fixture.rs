//! Replays a recorded engine state in the window, for comparing the desktop client with the
//! design and the Mac (`examples/design_audit.rs`). The state is a JSON file of the wire's own
//! shapes: workspaces, panes, the requests agents wait on, a transcript per agent pane, what
//! a terminal printed, the usage readings and the harness catalogue. Nothing is drawn that a
//! real engine could not send, and no engine is needed.

use crate::{
    app::{self, Setup},
    open_menu::Menu,
    theme::ThemeName,
};
use serde_json::Value;
use soyeht_client::{Engine, Event, Snapshot, streams::StreamOutput};
use std::path::PathBuf;

/// The events a fixture replays, in the order an engine would send them.
pub fn events(fixture: &Value) -> Result<Vec<Event>, String> {
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
        cursor: 1,
    };
    let mut events = vec![Event::Connected(snapshot)];
    if let Value::Object(transcripts) = section("transcripts") {
        for (pane_id, items) in transcripts {
            events.push(Event::Transcript {
                pane_id,
                items: serde_json::from_value(items).map_err(describe("transcripts"))?,
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
    events.push(Event::Usage(
        serde_json::from_value(Value::Array(list("usage"))).map_err(describe("usage"))?,
    ));
    events.push(Event::Harnesses(
        serde_json::from_value(Value::Array(list("harnesses"))).map_err(describe("harnesses"))?,
    ));
    Ok(events)
}

fn describe(section: &str) -> impl Fn(serde_json::Error) -> String + '_ {
    move |error| format!("the fixture's {section} are not the wire's shapes: {error}")
}

/// The menu a spec names: `new-pane`, `overflow`, `settings`, `harness:<pane>` or
/// `config:<pane>:<option>`.
pub fn menu(spec: &str) -> Option<Menu> {
    let mut parts = spec.splitn(3, ':');
    match (parts.next()?, parts.next(), parts.next()) {
        ("new-pane", None, None) => Some(Menu::NewPane),
        ("overflow", None, None) => Some(Menu::Overflow),
        ("settings", None, None) => Some(Menu::Settings),
        ("harness", Some(pane), None) => Some(Menu::Harness(pane.to_owned())),
        ("config", Some(pane), Some(config)) => Some(Menu::Config {
            pane_id: pane.to_owned(),
            config_id: config.to_owned(),
        }),
        _ => None,
    }
}

/// Opens the window on `fixture`, in `theme`, with the menu `open` names open (see [`menu`]),
/// in a window of `size` points when given, and runs until it closes.
pub fn run(
    fixture: &Value,
    theme: ThemeName,
    open: Option<&str>,
    size: Option<(f32, f32)>,
) -> Result<(), String> {
    let events = events(fixture)?;
    let menu = match open {
        Some(spec) => Some(menu(spec).ok_or_else(|| format!("no menu is named {spec}"))?),
        None => None,
    };
    app::launch(move || {
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
            menu,
            size,
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert_eq!(menu("nonsense"), None);
    }
}
