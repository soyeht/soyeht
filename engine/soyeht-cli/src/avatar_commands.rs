//! `soyeht avatar`: the frame tooling of an avatar (ADR 0166, slice 9a). It works with no engine:
//! it reads the image catalogue and the frame spec the program carries, runs one edit through the
//! provider the person chose, and checks one edited frame against its source.
//!
//! - `providers`: the image catalogue, and whether a key is stored for each provider.
//! - `frames`: the frame spec: the 18 frames and what each one changes.
//! - `frame`: makes one frame with a provider and writes it to a file.
//! - `check`: measures an edited frame against its source and says what is wrong with it.
//! - `key set PROVIDER` (the key on standard input) and `key clear PROVIDER`: the person's own key
//!   for a provider, kept on this computer. It is never printed and never on a command line.
//!
//! Which frames to make, in which order, and what to do with one that is wrong is the
//! `avatar-builder` skill's, not this program's.

use crate::args::{Flags, invalid};
use serde_json::{Value, json};
use soyeht_cli::Result;
use soyeht_image::{Edit, Picture, Rect, Vault};
use soyeht_platform::fs::{WriteMode, write_atomic};
use std::{
    io::Read as _,
    path::{Path, PathBuf},
    time::Duration,
};

/// The most of a key that is read from standard input.
const KEY_INPUT_BYTES: u64 = 2000;
const USAGE: &str = "avatar providers | avatar frames | \
avatar frame --provider ID --frame FRAME --source PNG --out PNG [--endpoint URL] [--model NAME] \
[--seed N] [--extra TEXT] [--timeout SECONDS] | \
avatar check --frame FRAME --source PNG --image PNG [--region X,Y,WIDTH,HEIGHT] | \
avatar key set PROVIDER | avatar key clear PROVIDER";

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
        ["key", "set", provider] => set_key(&directory, provider),
        ["key", "clear", provider] => clear_key(&directory, provider),
        _ => Err(invalid(USAGE)),
    }
}

fn providers(directory: &Path) -> Result<Value> {
    let vault = Vault::new(directory);
    let mut listed = Vec::new();
    for descriptor in soyeht_image::registry()? {
        let mut entry = serde_json::to_value(&descriptor)?;
        entry["keySet"] = json!(vault.read(&descriptor.id)?.is_some());
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
    let seed = match flags.get("--seed") {
        Some(seed) => seed
            .parse::<u64>()
            .map_err(|_| invalid("--seed must be a non-negative integer"))?,
        None => 1,
    };
    let seconds = match flags.number("--timeout")? {
        Some(seconds) => u64::from(seconds),
        None => descriptor.timeout_seconds,
    };
    let key = if descriptor.needs_key {
        Vault::new(directory).read(&descriptor.id)?
    } else {
        None
    };
    let provider = descriptor.id.clone();
    let choice = descriptor.choose(flags.get("--endpoint"), flags.get("--model"))?;
    let edit = Edit {
        source,
        prompt: &prompt,
        width,
        height,
        seed,
    };
    let picture = soyeht_image::edit(&choice, key.as_ref(), &edit, Duration::from_secs(seconds))?;
    write_atomic(out, &picture, WriteMode::KeepPermissions)?;
    Ok(json!({
        "frame": frame.id,
        "provider": provider,
        "out": out.display().to_string(),
        "width": width,
        "height": height,
        "bytes": picture.len(),
        "seed": seed,
    }))
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

fn set_key(directory: &Path, provider: &str) -> Result<Value> {
    if !soyeht_image::find(provider)?.needs_key {
        return Err(invalid("This provider does not use a key"));
    }
    let mut key = String::new();
    std::io::stdin()
        .take(KEY_INPUT_BYTES)
        .read_to_string(&mut key)?;
    Vault::new(directory).set(provider, &key)?;
    Ok(json!({"provider": provider, "keySet": true}))
}

fn clear_key(directory: &Path, provider: &str) -> Result<Value> {
    soyeht_image::find(provider)?;
    let cleared = Vault::new(directory).clear(provider)?;
    Ok(json!({"provider": provider, "keySet": false, "cleared": cleared}))
}
