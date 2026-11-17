//! The adapters: how one edit reaches a provider and how its picture comes back. Each speaks one
//! protocol (`Adapter`), and the catalogue entry names which. Nothing here knows a model; the
//! model name travels in the entry or in the person's choice.

mod comfy;
mod openai;

use crate::{
    catalogue::{Adapter, Choice},
    error::{Error, Result},
};
use soyeht_platform::secret::Secret;
use std::{path::Path, time::Duration};

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// One edit: the picture, what to change, the size of the result and a seed for the providers
/// that take one.
pub struct Edit<'a> {
    pub source: &'a Path,
    pub prompt: &'a str,
    pub width: u32,
    pub height: u32,
    pub seed: Option<u64>,
}

/// Everything one adapter needs for one edit.
pub(crate) struct Job<'a> {
    pub choice: &'a Choice,
    pub key: Option<&'a Secret>,
    pub edit: &'a Edit<'a>,
    pub timeout: Duration,
}

/// Runs one edit and returns the PNG the provider made. `key` is the person's own key for this
/// address (`Vault::key_for`); it is sent only to a provider whose entry needs one, and only over
/// https or to this computer.
pub fn edit(
    choice: &Choice,
    key: Option<&Secret>,
    edit: &Edit<'_>,
    timeout: Duration,
) -> Result<Vec<u8>> {
    let multiple = choice.descriptor.size_multiple.max(1);
    if !edit.width.is_multiple_of(multiple) || !edit.height.is_multiple_of(multiple) {
        return Err(Error::SourceSize {
            width: edit.width,
            height: edit.height,
            multiple,
        });
    }
    let key = match (choice.descriptor.needs_key, key) {
        (true, None) => return Err(Error::MissingKey),
        (true, Some(key)) => Some(key),
        (false, _) => None,
    };
    if key.is_some() && choice.endpoint.scheme() != "https" && !choice.is_loopback() {
        return Err(Error::InvalidEndpoint(
            "a key is sent only over https or to this computer",
        ));
    }
    let job = Job {
        choice,
        key,
        edit,
        timeout,
    };
    let picture = match choice.descriptor.adapter {
        Adapter::OpenaiImages => openai::edit(&job)?,
        Adapter::Comfyui => comfy::edit(&job)?,
    };
    if picture.starts_with(&PNG_SIGNATURE) {
        Ok(picture)
    } else {
        Err(Error::BadResponse("the picture is not a PNG"))
    }
}
