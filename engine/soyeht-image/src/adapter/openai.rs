//! The OpenAI image-edit request, which many services copy: a multipart `POST /images/edits`
//! with the picture, the model, the instruction and the size, answered with the picture as
//! base64.

use super::Job;
use crate::{
    error::{Error, Result},
    exchange::{self, invalid},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;

pub(super) fn edit(job: &Job<'_>) -> Result<Vec<u8>> {
    let model = job.choice.model.as_deref().ok_or(Error::NeedsModel)?;
    let url = job.choice.url(&["images", "edits"], &[])?;
    let mut request = exchange::request(&url, job.choice.https_only())?;
    if let Some(key) = job.key {
        request = request.bearer(key).map_err(invalid)?;
    }
    let size = format!("{}x{}", job.edit.width, job.edit.height);
    let request = request
        .form_file("image", job.edit.source, "avatar.png", "image/png")
        .and_then(|request| request.form_text("model", model))
        .and_then(|request| request.form_text("prompt", job.edit.prompt))
        .and_then(|request| request.form_text("size", &size))
        .map_err(invalid)?;
    let answer = exchange::send_json(request, job.key, job.timeout)?;
    let encoded = answer
        .pointer("/data/0/b64_json")
        .and_then(Value::as_str)
        .ok_or(Error::BadResponse("no picture"))?;
    STANDARD
        .decode(encoded)
        .map_err(|_| Error::BadResponse("the picture is not base64"))
}
