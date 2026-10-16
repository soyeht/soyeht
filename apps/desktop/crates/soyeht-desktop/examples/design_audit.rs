//! Opens the window on a recorded engine state, for comparing the desktop client with the
//! design file and the Mac. Usage:
//! `cargo run --features dev-tools --example design_audit -- <fixture.json> [theme] [--workspace id] [--open menu] [--size WxH]`
//! where `theme` is the end of a theme name without example (`milk`, `deepharbor`, ...) and
//! `menu` is `new-pane`, `overflow`, `settings` or `settings:<panel>`, `harness:<pane>`,
//! `loop:<pane>` or `config:<pane>:<option>`, the tasks surface: `tasks`, `tasks:list`,
//! `task:<id>` or `new-task`, or a place of the rail: `editor`, `terminal`, `agents`.
//! The editor scene is the workspace `ws-editor`: `--workspace ws-editor --open editor`.
//! `examples/design_audit.json` is the fixture the comparisons use.

use serde_json::{Value, json};
use soyeht_desktop::theme::ThemeName;

fn main() {
    if let Err(message) = open() {
        eprintln!("design_audit: {message}");
        std::process::exit(2);
    }
}

fn open() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("name a fixture file")?;
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
    let mut fixture: Value =
        serde_json::from_str(&text).map_err(|error| format!("{path}: {error}"))?;
    let (mut theme, mut open, mut size) = (ThemeName::DEFAULT, None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace" => {
                let workspace = args.next().ok_or("--workspace needs an id")?;
                fixture["focus"] = json!({"workspaceId": workspace});
            }
            "--open" => open = Some(args.next().ok_or("--open needs a menu")?),
            "--size" => size = Some(extent(&args.next().ok_or("--size needs WxH")?)?),
            name => theme = ThemeName::named(name).ok_or(format!("no theme named {name}"))?,
        }
    }
    soyeht_desktop::run_fixture(&fixture, theme, open.as_deref(), size)
}

/// `1280x800` as a width and a height in points.
fn extent(text: &str) -> Result<(f32, f32), String> {
    let (width, height) = text.split_once('x').ok_or("a size is WxH")?;
    match (width.parse(), height.parse()) {
        (Ok(width), Ok(height)) => Ok((width, height)),
        _ => Err(format!("{text} is not a size")),
    }
}
