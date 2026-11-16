//! The OpenAI image-edit request, which many services copy: a multipart `POST /images/edits`
//! with the picture, the model, the instruction and the size, answered with the picture as
//! base64 (or an address to fetch it from).

use super::Job;
use crate::{
    curl::Request,
    error::{Error, Result},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;
use url::Url;

pub(super) fn edit(job: &Job<'_>) -> Result<Vec<u8>> {
    let model = job.choice.model.as_deref().ok_or(Error::NeedsModel)?;
    let url = job.choice.url("images/edits");
    let mut request = Request::new(&url, job.choice.https_only())?;
    if let Some(key) = job.key {
        request = request.bearer(key)?;
    }
    let size = format!("{}x{}", job.edit.width, job.edit.height);
    let answer = request
        .png_field("image", job.edit.source, "avatar.png")?
        .field("model", model)?
        .field("prompt", job.edit.prompt)?
        .field("size", &size)?
        .send(job.timeout)?
        .json()?;
    let image = answer
        .pointer("/data/0")
        .ok_or(Error::BadResponse("no picture"))?;
    if let Some(encoded) = image.get("b64_json").and_then(Value::as_str) {
        return STANDARD
            .decode(encoded)
            .map_err(|_| Error::BadResponse("the picture is not base64"));
    }
    // The address is the service's, not the person's: it is fetched over https, with no key.
    let address = image
        .get("url")
        .and_then(Value::as_str)
        .ok_or(Error::BadResponse("no picture"))?;
    if !Url::parse(address).is_ok_and(|address| address.scheme() == "https") {
        return Err(Error::BadResponse("the picture's address is not https"));
    }
    Request::new(address, true)?.send(job.timeout)?.success()
}
