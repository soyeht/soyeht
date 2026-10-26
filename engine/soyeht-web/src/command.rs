use soyeht_protocol::{
    WebCommandKind, WebCommandParams, WebInteractionMode, WebViewport, WebViewportPreset, limits,
};

use crate::{Error, Result};

pub const DEFAULT_ZOOM_PERCENT: u32 = 100;

pub fn viewport(
    preset: WebViewportPreset,
    width: Option<u32>,
    height: Option<u32>,
    rotate: bool,
    zoom_percent: u32,
) -> Result<WebViewport> {
    if !(25..=500).contains(&zoom_percent) {
        return Err(Error::InvalidViewport);
    }
    let (mut width, mut height) = match preset {
        WebViewportPreset::IphoneSe => (375, 667),
        WebViewportPreset::Iphone15Pro => (393, 852),
        WebViewportPreset::Ipad => (768, 1024),
        WebViewportPreset::Responsive => (
            width.ok_or(Error::InvalidViewport)?,
            height.ok_or(Error::InvalidViewport)?,
        ),
    };
    if width == 0
        || height == 0
        || width as usize > limits::WEB_EXTENT_MAX
        || height as usize > limits::WEB_EXTENT_MAX
    {
        return Err(Error::InvalidViewport);
    }
    if rotate {
        std::mem::swap(&mut width, &mut height);
    }
    Ok(WebViewport {
        width,
        height,
        preset,
        zoom_percent,
    })
}

pub fn validate_command(params: &WebCommandParams, mode: WebInteractionMode) -> Result<()> {
    if mode == WebInteractionMode::Human {
        return Err(Error::HumanModeUnavailable);
    }
    if params.pane_id.is_empty()
        || params.operation_key.is_empty()
        || params.timeout_ms as usize > limits::WEB_WAIT_TIMEOUT_MS
    {
        return Err(Error::InvalidCommand);
    }
    let bytes = serde_json::to_vec(&params.args)
        .map_err(|_| Error::InvalidCommand)?
        .len();
    if bytes > limits::JSON_BYTES {
        return Err(Error::PayloadTooLarge);
    }
    if params.command == WebCommandKind::Wait {
        validate_wait(&params.args, params.timeout_ms)?;
    }
    if params.command == WebCommandKind::Evaluate {
        validate_evaluate(&params.args)?;
    }
    Ok(())
}

pub const fn command_response_budget(command: &WebCommandKind) -> usize {
    match command {
        WebCommandKind::Snapshot => limits::WEB_SNAPSHOT_BYTES,
        WebCommandKind::Evaluate => limits::WEB_EVALUATE_BYTES,
        WebCommandKind::Screenshot => limits::WEB_SCREENSHOT_BYTES,
        WebCommandKind::Read => limits::WEB_READ_BYTES,
        _ => limits::JSON_BYTES,
    }
}

fn validate_wait(args: &serde_json::Value, timeout_ms: u32) -> Result<()> {
    let Some(object) = args.as_object() else {
        return Err(Error::InvalidWait);
    };
    let text = object.get("text").and_then(serde_json::Value::as_str);
    let selector = object.get("selector").and_then(serde_json::Value::as_str);
    let network_idle = object
        .get("networkIdle")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let has_condition = text
        .is_some_and(|value| !value.is_empty() && value.len() <= limits::WEB_FIELD_BYTES)
        || selector
            .is_some_and(|value| !value.is_empty() && value.len() <= limits::WEB_FIELD_BYTES)
        || network_idle;
    if has_condition && timeout_ms as usize <= limits::WEB_WAIT_TIMEOUT_MS {
        Ok(())
    } else {
        Err(Error::InvalidWait)
    }
}

fn validate_evaluate(args: &serde_json::Value) -> Result<()> {
    let script = args
        .as_object()
        .and_then(|object| object.get("js"))
        .and_then(serde_json::Value::as_str)
        .ok_or(Error::InvalidCommand)?;
    if !script.is_empty() && script.len() <= limits::WEB_EVALUATE_BYTES {
        Ok(())
    } else {
        Err(Error::PayloadTooLarge)
    }
}
