//! `soyeht avatar`: the frame tooling of an avatar (ADR 0166, slice 9a). It works with no engine:
//! it reads the image catalogue and the frame spec the program carries, runs one edit through the
//! provider the person chose, and checks one edited frame against its source.
//!
//! - `providers`: the image catalogue, and whether a key is stored for each provider.
//! - `frames`: the frame spec: the 18 frames and what each one changes.
//! - `frame`: makes one frame with a provider and writes it to a file.
//! - `check`: measures an edited frame against its source and says what is wrong with it.
//! - `key set PROVIDER [--endpoint URL]` (the key on one line of standard input) and `key clear
//!   PROVIDER [--endpoint URL]`: the person's own key for a provider **at one address**, kept on
//!   this computer. A call to another address does not find it. It is never printed and never on
//!   a command line.
//!
//! Which frames to make, in which order, and what to do with one that is wrong is the
//! `avatar-builder` skill's, not this program's.

use crate::args::{Flags, invalid};
use serde_json::{Value, json};
use soyeht_cli::Result;
use soyeht_image::{Edit, Picture, Rect, Vault};
use soyeht_platform::fs::{WriteMode, write_atomic};
use std::{
    io::BufRead as _,
    io::Read as _,
    path::{Path, PathBuf},
    time::Duration,
};

/// The most of a key that is read from standard input: one line.
const KEY_INPUT_BYTES: u64 = 2000;
const USAGE: &str = "avatar providers | avatar frames | \
avatar frame --provider ID --frame FRAME --source PNG --out PNG [--endpoint URL] [--model NAME] \
[--seed N] [--extra TEXT] [--timeout SECONDS] | \
avatar check --frame FRAME --source PNG --image PNG [--region X,Y,WIDTH,HEIGHT] | \
avatar key set PROVIDER [--endpoint URL] | avatar key clear PROVIDER [--endpoint URL]";

pub fn execute(directory: PathBuf, args: &[String]) -> Result<Value> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["providers"] => providers(&directory),
        ["frames"] => frames(),
        ["frame", rest @ ..] => frame(&directory, rest),
        ["check", rest @ ..] => check(rest),
        ["key", "set", provider, rest @ ..] => set_key(&directory, provider, rest),
        ["key", "clear", provider, rest @ ..] => clear_key(&directory, provider, rest),
        _ => Err(invalid(USAGE)),
    }
}

fn providers(directory: &Path) -> Result<Value> {
    let vault = Vault::new(directory);
    let mut listed = Vec::new();
    for descriptor in soyeht_image::registry()? {
        let mut entry = serde_json::to_value(&descriptor)?;
        let hosts = vault.hosts(&descriptor.id)?;
        entry["keySet"] = json!(!hosts.is_empty());
        entry["keyHosts"] = json!(hosts);
        entry["takesModel"] = json!(descriptor.adapter.takes_model());
        entry["takesSeed"] = json!(descriptor.adapter.takes_seed());
        listed.push(entry);
    }
    Ok(json!({"providers": listed}))
}

fn frames() -> Result<Value> {
    Ok(json!({"frames": soyeht_image::spec()?.frames}))
}

fn frame(directory: &Path, rest: &[&str]) -> Result<Value> {
    let flags = Flags::parse(
        rest,
        &[
            "--provider",
            "--frame",
            "--source",
            "--out",
            "--endpoint",
            "--model",
            "--seed",
            "--extra",
            "--timeout",
        ],
    )?;
    let descriptor = soyeht_image::find(flags.required("--provider")?)?;
    let spec = soyeht_image::spec()?;
    let frame = spec.frame(flags.required("--frame")?)?;
    let prompt = spec.prompt(frame, flags.get("--extra"))?;
    let source = Path::new(flags.required("--source")?);
    let out = Path::new(flags.required("--out")?);
    if same_file(source, out) {
        return Err(invalid("--out must not be the source picture"));
    }
    let (width, height) = Picture::read(source)?.size();
    let takes_seed = descriptor.adapter.takes_seed();
    let seed = match (flags.get("--seed"), takes_seed) {
        (Some(_), false) => return Err(soyeht_image::Error::NotUsed("seed").into()),
        (Some(seed), true) => Some(
            seed.parse::<u64>()
                .map_err(|_| invalid("--seed must be a non-negative integer"))?,
        ),
        (None, takes_seed) => takes_seed.then_some(1),
    };
    let seconds = match flags.number("--timeout")? {
        Some(seconds) => u64::from(seconds),
        None => descriptor.timeout_seconds,
    };
    let (provider, needs_key) = (descriptor.id.clone(), descriptor.needs_key);
    let choice = descriptor.choose(flags.get("--endpoint"), flags.get("--model"))?;
    let key = if needs_key {
        Some(Vault::new(directory).key_for(&provider, &choice.key_host())?)
    } else {
        None
    };
    let edit = Edit {
        source,
        prompt: &prompt,
        width,
        height,
        seed,
    };
    let picture = soyeht_image::edit(&choice, key.as_ref(), &edit, Duration::from_secs(seconds))?;
    write_atomic(out, &picture, WriteMode::KeepPermissions)?;
    let mut made = json!({
        "frame": frame.id,
        "provider": provider,
        "out": out.display().to_string(),
        "width": width,
        "height": height,
        "bytes": picture.len(),
    });
    if let Some(seed) = seed {
        made["seed"] = json!(seed);
    }
    Ok(made)
}

/// Whether `out` names the file `source` already is (a missing `out` is a new file).
fn same_file(source: &Path, out: &Path) -> bool {
    match (
        soyeht_platform::fs::canonicalize(source),
        soyeht_platform::fs::canonicalize(out),
    ) {
        (Ok(source), Ok(out)) => source == out,
        _ => false,
    }
}

fn check(rest: &[&str]) -> Result<Value> {
    let flags = Flags::parse(rest, &["--frame", "--source", "--image", "--region"])?;
    let spec = soyeht_image::spec()?;
    let frame = spec.frame(flags.required("--frame")?)?;
    let source = Picture::read(Path::new(flags.required("--source")?))?;
    let image = Picture::read(Path::new(flags.required("--image")?))?;
    let region = flags.get("--region").map(region).transpose()?;
    let report = soyeht_image::check(&spec, frame, &source, &image, region)?;
    Ok(serde_json::to_value(report)?)
}

/// `X,Y,WIDTH,HEIGHT` in pixels.
fn region(text: &str) -> Result<Rect> {
    let numbers: Vec<u32> = text
        .split(',')
        .map(|part| part.trim().parse::<u32>())
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| invalid("--region is X,Y,WIDTH,HEIGHT in pixels"))?;
    match numbers.as_slice() {
        &[x, y, width, height] if width > 0 && height > 0 => Ok(Rect {
            x,
            y,
            width,
            height,
        }),
        _ => Err(invalid("--region is X,Y,WIDTH,HEIGHT in pixels")),
    }
}

fn set_key(directory: &Path, provider: &str, rest: &[&str]) -> Result<Value> {
    let descriptor = soyeht_image::find(provider)?;
    if !descriptor.needs_key {
        return Err(invalid("This provider does not use a key"));
    }
    let flags = Flags::parse(rest, &["--endpoint"])?;
    let host = descriptor.key_host(flags.get("--endpoint"))?;
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .take(KEY_INPUT_BYTES)
        .read_line(&mut line)?;
    Vault::new(directory).set(provider, &host, &line)?;
    Ok(json!({"provider": provider, "address": host, "keySet": true}))
}

/// Forgets the key at the named address, or every key of the provider when none is named. The
/// provider need not be in the catalogue any more.
fn clear_key(directory: &Path, provider: &str, rest: &[&str]) -> Result<Value> {
    let flags = Flags::parse(rest, &["--endpoint"])?;
    let host = flags
        .get("--endpoint")
        .map(soyeht_image::host_of)
        .transpose()?;
    let cleared = Vault::new(directory).clear(provider, host.as_deref())?;
    Ok(json!({"provider": provider, "cleared": cleared}))
}
