//! The frame spec (ADR 0166, section 4): the 18 pictures an avatar is built from, as data
//! (`data/frames.json`). A frame has an id (the ids of the avatar package: the eight mouth
//! shapes, the blink, the seven eye frames, and the two resting mouths), a group, and the change
//! it asks for. A group holds the prompt template, what must stay the same, and how much of the
//! picture may change. The prompt of one edit is built here and nowhere else.
//!
//! The thresholds are the check's (`check.rs`); they are data so they can be tuned from real
//! outputs without touching code.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const SPEC: &str = include_str!("../data/frames.json");

/// How `check` measures a change.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Measure {
    /// A pixel changed when one of its channels moved by more than this (0 to 255).
    pub pixel_delta: u8,
    /// The side, in pixels, of the squares the picture is cut into.
    pub block: u32,
    /// A square counts as changed when this share of its pixels changed.
    pub active_share: f64,
}

/// What every frame of one kind shares.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Group {
    /// The instruction, with `{change}` and `{keep}` where the frame's change and the group's
    /// unchanged parts go.
    pub template: String,
    pub keep: String,
    /// The least and the most of the picture, as a share of its area, the change may cover.
    pub min_change: f64,
    pub max_change: f64,
}

/// One picture of the set.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub id: String,
    pub group: String,
    /// What changes, as a clause that follows "so that".
    pub change: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub check: Measure,
    pub groups: BTreeMap<String, Group>,
    pub frames: Vec<Frame>,
}

/// The spec the build embeds.
pub fn spec() -> Result<Spec> {
    let spec: Spec = serde_json::from_str(SPEC).map_err(|_| Error::Data("frames.json"))?;
    if spec
        .frames
        .iter()
        .any(|frame| !spec.groups.contains_key(&frame.group))
    {
        return Err(Error::Data("a frame names a group that does not exist"));
    }
    Ok(spec)
}

impl Spec {
    pub fn frame(&self, id: &str) -> Result<&Frame> {
        self.frames
            .iter()
            .find(|frame| frame.id == id)
            .ok_or_else(|| Error::UnknownFrame(id.to_owned()))
    }

    /// The group of `frame` (`spec` checked that it exists).
    pub fn group(&self, frame: &Frame) -> Result<&Group> {
        self.groups
            .get(&frame.group)
            .ok_or(Error::Data("a frame names a group that does not exist"))
    }

    /// The instruction for one edit: the group's template with the frame's change, then `extra`
    /// when the agent adds a correction after a frame was rejected.
    pub fn prompt(&self, frame: &Frame, extra: Option<&str>) -> Result<String> {
        let group = self.group(frame)?;
        let mut text = group
            .template
            .replace("{change}", &frame.change)
            .replace("{keep}", &group.keep);
        if let Some(extra) = extra.map(str::trim).filter(|extra| !extra.is_empty()) {
            text.push(' ');
            text.push_str(extra);
        }
        Ok(text)
    }
}
