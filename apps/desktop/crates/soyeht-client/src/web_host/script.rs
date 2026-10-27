//! The scripts the host runs in the page's isolated world (ADR 0110), and the conditions of a
//! wait. Each script is a function the page cannot see, called with numbers and quoted text only.

use super::{HostError, Page};
use serde_json::Value;
use soyeht_protocol::limits;

const SNAPSHOT: &str = include_str!("js/snapshot.js");

/// The most nodes one snapshot reads before it says it is cut short; the byte budget of the
/// contract cuts it earlier on a long page.
const MAX_NODES: usize = 3000;

/// The script that reads the page into `{nodes, truncated}`, as JSON text.
pub(super) fn snapshot_call(generation: u64) -> String {
    format!(
        "{}({{maxNodes:{MAX_NODES},labelChars:{},fieldChars:{},tokenChars:{},extentMax:{}}},{generation})",
        SNAPSHOT.trim_end(),
        limits::WEB_LABEL_BYTES,
        limits::WEB_FIELD_BYTES,
        limits::WEB_TOKEN_BYTES,
        limits::WEB_EXTENT_MAX,
    )
}

/// The script of a scroll: by the page's own scrolling, which the page sees as the scroll of a
/// person using a keyboard or a wheel, not as an event of ours.
pub(super) fn scroll(args: &Value) -> String {
    let amount = |name: &str| {
        args.get(name)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .unwrap_or(0.0)
            .clamp(-100_000.0, 100_000.0)
    };
    let reference = args
        .get("ref")
        .and_then(Value::as_str)
        .map_or_else(|| "null".to_owned(), quote);
    format!(
        "(function(a){{const refs=globalThis.__soyehtReferences;let target=window;\
         if(a.ref!==null){{const element=refs&&refs.get(a.ref);\
         if(!element||!element.isConnected)return 'stale';target=element;}}\
         target.scrollBy({{left:a.dx,top:a.dy,behavior:'instant'}});return 'ok';}})\
         ({{ref:{reference},dx:{},dy:{}}})",
        amount("dx"),
        amount("dy"),
    )
}

/// Runs a script of the host; what it returns is text.
pub(super) fn run(page: &dyn Page, source: &str) -> Result<String, HostError> {
    let answer = page.run_script(source)?;
    if answer == "stale" {
        return Err(HostError::Stale("Take a new snapshot and try again."));
    }
    Ok(answer)
}

/// What a `web_wait` waits for. Every condition given must hold.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Wait {
    text: Option<String>,
    selector: Option<String>,
    network_idle: bool,
}

impl Wait {
    pub fn from_args(args: &Value) -> Self {
        let text = |name: &str| {
            args.get(name)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        Self {
            text: text("text"),
            selector: text("selector"),
            network_idle: args.get("networkIdle").and_then(Value::as_bool) == Some(true),
        }
    }

    /// Whether the page meets the conditions now. `idle`: the page is done loading and has been
    /// quiet for a moment, which is all the host can tell of the network without a script in
    /// the page, and the page gets none.
    pub fn holds(&self, page: &dyn Page, idle: bool) -> Result<bool, HostError> {
        if (self.network_idle || self.is_empty()) && !idle {
            return Ok(false);
        }
        if let Some(text) = &self.text {
            let source = format!(
                "String(document.body?document.body.innerText:'').includes({})",
                quote(text)
            );
            if page.run_script(&source)? != "true" {
                return Ok(false);
            }
        }
        if let Some(selector) = &self.selector {
            let source = format!("String(document.querySelector({})!==null)", quote(selector));
            if page.run_script(&source)? != "true" {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn is_empty(&self) -> bool {
        self.text.is_none() && self.selector.is_none() && !self.network_idle
    }
}

/// `text` as a JavaScript string literal.
fn quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\u{2028}' | '\u{2029}' | '<' => {
                quoted.push_str(&format!("\\u{:04x}", u32::from(character)));
            }
            control if u32::from(control) < 0x20 => {
                quoted.push_str(&format!("\\u{:04x}", u32::from(control)));
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_snapshot_call_carries_only_numbers_from_the_contract() {
        let call = snapshot_call(7);
        assert!(call.ends_with(&format!(
            "({{maxNodes:3000,labelChars:{},fieldChars:{},tokenChars:{},extentMax:{}}},7)",
            limits::WEB_LABEL_BYTES,
            limits::WEB_FIELD_BYTES,
            limits::WEB_TOKEN_BYTES,
            limits::WEB_EXTENT_MAX
        )));
        assert!(call.starts_with("// Reads the page"));
    }

    /// A reading script that dispatched an event or added a node would be a trace a page can
    /// find. The snapshot builds its result in memory and returns text.
    #[test]
    fn the_scripts_touch_nothing_in_the_page() {
        for forbidden in [
            "dispatchEvent",
            "appendChild",
            "createElement",
            "insertBefore",
            "setAttribute",
            "addEventListener",
            ".click(",
            ".focus(",
            "document.write",
        ] {
            assert!(!SNAPSHOT.contains(forbidden), "{forbidden}");
        }
        let scroll = scroll(&json!({"dx": 0, "dy": 300}));
        for forbidden in [
            "dispatchEvent",
            "appendChild",
            "createElement",
            "setAttribute",
        ] {
            assert!(!scroll.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn text_goes_into_a_script_as_a_string_literal_that_cannot_end_early() {
        assert_eq!(quote("a\"b\\c\nd"), r#""a\"b\\c\nd""#);
        assert_eq!(quote("</script>"), "\"\\u003c/script>\"");
        assert_eq!(quote("x\u{2028}y"), "\"x\\u2028y\"");
        assert_eq!(quote("\u{1}"), "\"\\u0001\"");
        assert_eq!(quote("ação"), "\"ação\"");
    }

    #[test]
    fn a_scroll_is_a_bounded_move_of_the_window_or_of_a_referenced_element() {
        let window = scroll(&json!({"dx": 5.5, "dy": -300}));
        assert!(window.ends_with("({ref:null,dx:5.5,dy:-300})"), "{window}");
        let element = scroll(&json!({"ref": "e12", "dy": 1.0e12}));
        assert!(
            element.ends_with("({ref:\"e12\",dx:0,dy:100000})"),
            "{element}"
        );
    }

    #[test]
    fn a_wait_names_its_conditions() {
        let wait = Wait::from_args(&json!({"text": "ready", "selector": "", "networkIdle": true}));
        assert_eq!(
            wait,
            Wait {
                text: Some("ready".into()),
                selector: None,
                network_idle: true
            }
        );
        assert!(Wait::from_args(&json!({})).is_empty());
    }
}
