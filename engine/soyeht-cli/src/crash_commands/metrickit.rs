//! A crash diagnostic of the system (`MXCrashDiagnostic.jsonRepresentation()`) as a report.
//!
//! Only a fixed set of facts is read: the signal, the exception type, the program's version and
//! architecture, and for each frame of the stack the module's name and the offset in it. The
//! termination reason and the other free texts are left out, and so is any path.

use serde_json::Value;
use soyeht_platform::crash::{LINE_CHARS, MAX_FRAMES, Program, Report};

/// A version or architecture name is at most this long, and only these characters.
const NAME_CHARS: usize = 32;

/// The report of a crash diagnostic, or `None` when `diagnostic` is not one (it has neither the
/// metadata nor the call stack tree of `MXCrashDiagnostic`).
pub fn report(diagnostic: &Value, time: u64) -> Option<Report> {
    if !diagnostic["diagnosticMetaData"].is_object() && !diagnostic["callStackTree"].is_object() {
        return None;
    }
    let meta = &diagnostic["diagnosticMetaData"];
    Some(Report {
        version: name(&meta["appVersion"]).unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned()),
        system: format!(
            "macos {}",
            name(&meta["platformArchitecture"])
                .unwrap_or_else(|| std::env::consts::ARCH.to_owned())
        ),
        program: Program::Mac,
        time,
        pid: 0,
        thread: "attributed".to_owned(),
        fatal: true,
        kind: kind(meta),
        location: String::new(),
        frames: frames(&diagnostic["callStackTree"]),
        log: Vec::new(),
    })
}

/// A short name from the system's text: letters, digits and `.+-_` only.
fn name(value: &Value) -> Option<String> {
    let text: String = value
        .as_str()?
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || ".+-_".contains(*c))
        .take(NAME_CHARS)
        .collect();
    (!text.is_empty()).then_some(text)
}

fn kind(meta: &Value) -> String {
    if let Some(signal) = meta["signal"].as_u64() {
        let label = match signal {
            4 => "SIGILL",
            5 => "SIGTRAP",
            6 => "SIGABRT",
            8 => "SIGFPE",
            10 => "SIGBUS",
            11 => "SIGSEGV",
            _ => return format!("signal_{signal}"),
        };
        return format!("signal_{label}");
    }
    match meta["exceptionType"].as_u64() {
        Some(exception) => format!("exception_{exception}"),
        None => "crash".to_owned(),
    }
}

/// The frames of the stack of the crashed thread, innermost first. The tree's root is the
/// outermost caller; the first sub-frame of each frame is followed down to the frame that crashed.
fn frames(tree: &Value) -> Vec<String> {
    let stacks = tree["callStacks"].as_array().map_or(&[][..], Vec::as_slice);
    let stack = stacks
        .iter()
        .find(|stack| stack["threadAttributed"].as_bool() == Some(true))
        .or_else(|| stacks.first());
    let mut frames = Vec::new();
    let mut frame = stack.and_then(|stack| stack["callStackRootFrames"].as_array()?.first());
    while let Some(current) = frame {
        if frames.len() >= MAX_FRAMES * 4 {
            break;
        }
        if let Some(offset) = current["offsetIntoBinaryTextSegment"].as_u64() {
            let module = current["binaryName"]
                .as_str()
                .and_then(|path| path.rsplit(['/', '\\']).next())
                .and_then(|module| name(&Value::from(module)))
                .unwrap_or_else(|| "unknown".to_owned());
            frames.push(format!("{module}+{offset:#x}"));
        }
        frame = current["subFrames"].as_array().and_then(|sub| sub.first());
    }
    frames.reverse();
    frames.truncate(MAX_FRAMES);
    frames
        .into_iter()
        .map(|frame| frame.chars().take(LINE_CHARS).collect())
        .collect()
}
