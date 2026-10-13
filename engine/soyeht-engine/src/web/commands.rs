use crate::{core::Core, dispatch::Outcome, errors, errors::Result};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use soyeht_protocol::{
    WebClickParams, WebCommandKind, WebCommandParams, WebConsoleParams, WebEvaluateParams,
    WebHoverParams, WebNetworkParams, WebPressParams, WebResizeParams, WebScreenshotParams,
    WebScrollParams, WebSelectParams, WebSnapshotParams, WebTypeParams, WebViewportPreset,
    WebWaitParams, method,
};
use soyeht_web::{DEFAULT_ZOOM_PERCENT, viewport};
use uuid::Uuid;

const DEFAULT_COMMAND_TIMEOUT_MS: u32 = 30_000;

pub(crate) fn handles(method: &str) -> bool {
    matches!(
        method,
        method::WEB_SNAPSHOT
            | method::WEB_CLICK
            | method::WEB_TYPE
            | method::WEB_PRESS
            | method::WEB_HOVER
            | method::WEB_SELECT
            | method::WEB_SCROLL
            | method::WEB_WAIT
            | method::WEB_EVALUATE
            | method::WEB_SCREENSHOT
            | method::WEB_CONSOLE
            | method::WEB_NETWORK
            | method::WEB_RESIZE
    )
}

impl Core {
    pub(crate) fn web_command(
        &mut self,
        connection: &str,
        request_id: Value,
        method: &str,
        params: &Value,
    ) -> Result<Outcome> {
        let canonical = canonical_params(method, params)?;
        validate_resize(method, &canonical)?;
        let pane_id = errors::param_str(&canonical, "paneId")?.to_owned();
        let timeout_ms = canonical
            .get("timeoutMs")
            .and_then(Value::as_u64)
            .map(u32::try_from)
            .transpose()
            .map_err(|_| errors::invalid("Invalid web command timeout"))?
            .unwrap_or(DEFAULT_COMMAND_TIMEOUT_MS);
        let operation_key = canonical
            .get("idempotencyKey")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let args = command_args(&canonical)?;
        let command = WebCommandParams {
            pane_id,
            command: command_kind(method)?,
            args,
            timeout_ms,
            operation_key,
        };
        self.queue_web_command(connection, request_id, method, &canonical, command)?;
        Ok(Outcome::Queued)
    }
}

fn canonical_params(method: &str, params: &Value) -> Result<Value> {
    match method {
        method::WEB_SNAPSHOT => canonical::<WebSnapshotParams>(params),
        method::WEB_CLICK => canonical::<WebClickParams>(params),
        method::WEB_TYPE => canonical::<WebTypeParams>(params),
        method::WEB_PRESS => canonical::<WebPressParams>(params),
        method::WEB_HOVER => canonical::<WebHoverParams>(params),
        method::WEB_SELECT => canonical::<WebSelectParams>(params),
        method::WEB_SCROLL => canonical::<WebScrollParams>(params),
        method::WEB_WAIT => canonical::<WebWaitParams>(params),
        method::WEB_EVALUATE => canonical::<WebEvaluateParams>(params),
        method::WEB_SCREENSHOT => canonical::<WebScreenshotParams>(params),
        method::WEB_CONSOLE => canonical::<WebConsoleParams>(params),
        method::WEB_NETWORK => canonical::<WebNetworkParams>(params),
        method::WEB_RESIZE => canonical::<WebResizeParams>(params),
        _ => Err(errors::invalid("Unsupported web command")),
    }
}

fn canonical<T: DeserializeOwned + Serialize>(params: &Value) -> Result<Value> {
    let typed: T = serde_json::from_value(params.clone())
        .map_err(|_| errors::invalid("Invalid web command parameters"))?;
    serde_json::to_value(typed)
        .map_err(|_| errors::internal("Web command parameter serialization failed"))
}

fn command_args(params: &Value) -> Result<Value> {
    let mut args = params
        .as_object()
        .cloned()
        .ok_or_else(|| errors::invalid("Web command parameters must be an object"))?;
    args.remove("paneId");
    args.remove("idempotencyKey");
    args.remove("timeoutMs");
    Ok(Value::Object(args))
}

fn command_kind(method: &str) -> Result<WebCommandKind> {
    match method {
        method::WEB_SNAPSHOT => Ok(WebCommandKind::Snapshot),
        method::WEB_CLICK => Ok(WebCommandKind::Click),
        method::WEB_TYPE => Ok(WebCommandKind::Type),
        method::WEB_PRESS => Ok(WebCommandKind::Press),
        method::WEB_HOVER => Ok(WebCommandKind::Hover),
        method::WEB_SELECT => Ok(WebCommandKind::Select),
        method::WEB_SCROLL => Ok(WebCommandKind::Scroll),
        method::WEB_WAIT => Ok(WebCommandKind::Wait),
        method::WEB_EVALUATE => Ok(WebCommandKind::Evaluate),
        method::WEB_SCREENSHOT => Ok(WebCommandKind::Screenshot),
        method::WEB_CONSOLE => Ok(WebCommandKind::Console),
        method::WEB_NETWORK => Ok(WebCommandKind::Network),
        method::WEB_RESIZE => Ok(WebCommandKind::Resize),
        _ => Err(errors::invalid("Unsupported web command")),
    }
}

fn validate_resize(method: &str, params: &Value) -> Result<()> {
    if method != method::WEB_RESIZE {
        return Ok(());
    }
    let request: WebResizeParams = serde_json::from_value(params.clone())
        .map_err(|_| errors::invalid("Invalid web resize parameters"))?;
    let preset = match (request.preset, request.width, request.height) {
        (Some(preset), None, None) => preset,
        (None, Some(_), Some(_)) => WebViewportPreset::Responsive,
        _ => {
            return Err(errors::invalid(
                "Web resize requires one preset or width and height",
            ));
        }
    };
    viewport(
        preset,
        request.width,
        request.height,
        request.rotate.unwrap_or(false),
        DEFAULT_ZOOM_PERCENT,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_commands_drop_unknown_and_transport_only_arguments() {
        let canonical = canonical_params(
            "web.type",
            &json!({"paneId":"p","ref":"e1","text":"hi","submit":true,
                "idempotencyKey":"k","unknown":"drop"}),
        )
        .unwrap();
        assert_eq!(
            command_args(&canonical).unwrap(),
            json!({"ref":"e1","text":"hi","submit":true})
        );
        assert!(canonical.get("unknown").is_none());
    }

    #[test]
    fn resize_requires_exactly_a_preset_or_a_custom_size() {
        for valid in [
            json!({"paneId":"p","preset":"ipad","idempotencyKey":"a"}),
            json!({"paneId":"p","width":800,"height":600,"idempotencyKey":"b"}),
        ] {
            assert!(validate_resize("web.resize", &valid).is_ok());
        }
        for invalid in [
            json!({"paneId":"p","idempotencyKey":"c"}),
            json!({"paneId":"p","width":800,"idempotencyKey":"d"}),
            json!({"paneId":"p","preset":"ipad","width":800,"height":600,
                "idempotencyKey":"e"}),
        ] {
            assert!(validate_resize("web.resize", &invalid).is_err());
        }
    }
}
