#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `soyeht avatar`: the commands run with no engine, against a data directory of their own.

use serde_json::Value;
use soyeht_avatar::{EYE_IDS, Shape};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    thread,
};

/// A folder of the test, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        Self(soyeht_platform::testing::scratch_dir("avatar-cli"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// An RGBA picture whose pixel at `(x, y)` is `pixel(x, y)`.
fn rgba_png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let data: Vec<u8> = (0..width * height)
        .flat_map(|at| pixel(at % width, at / width))
        .collect();
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&data).unwrap();
    writer.finish().unwrap();
    bytes
}

/// Runs `soyeht --data-dir DIR avatar ARGS` with `stdin` as its standard input.
fn avatar(data: &Path, args: &[&str], stdin: &str) -> (bool, Value, String) {
    let input = data.join("stdin.txt");
    fs::write(&input, stdin).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_soyeht"))
        .arg("--data-dir")
        .arg(data)
        .arg("avatar")
        .args(args)
        .stdin(fs::File::open(&input).unwrap())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
    (output.status.success(), value, stderr)
}

fn ids(frames: &Value, group: &str) -> BTreeSet<String> {
    frames["frames"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|frame| frame["group"] == group)
        .map(|frame| frame["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_frame_ids_are_the_ids_of_the_avatar_package() {
    let data = Scratch::new();
    let (ok, frames, _) = avatar(&data.0, &["frames"], "");
    assert!(ok);
    assert_eq!(frames["frames"].as_array().unwrap().len(), 18);
    let mouths: BTreeSet<String> = Shape::ALL
        .iter()
        .filter(|shape| **shape != Shape::Rest)
        .map(|shape| shape.id().to_owned())
        .collect();
    assert_eq!(ids(&frames, "mouth"), mouths);
    let eyes: BTreeSet<String> = EYE_IDS.iter().map(|id| (*id).to_owned()).collect();
    let spec_eyes: BTreeSet<String> = ids(&frames, "eyes")
        .union(&ids(&frames, "blink"))
        .cloned()
        .collect();
    assert_eq!(spec_eyes, eyes);
    assert_eq!(ids(&frames, "resting").len(), 2);
}

#[test]
fn the_catalogue_lists_providers_and_the_person_stores_and_clears_a_key_without_it_being_shown() {
    let data = Scratch::new();
    let (ok, listed, stderr) = avatar(&data.0, &["providers"], "");
    assert!(ok, "{stderr}");
    let providers = listed["providers"].as_array().unwrap();
    let cloud = providers.iter().find(|p| p["locality"] == "cloud").unwrap();
    let id = cloud["id"].as_str().unwrap();
    assert!(providers.iter().all(|p| p["keySet"] == false));
    assert!(providers.iter().all(|p| p.get("recipe").is_none()));

    let (ok, stored, stderr) = avatar(&data.0, &["key", "set", id], "sk-never-shown\n");
    assert!(ok, "{stderr}");
    assert!(!stored.to_string().contains("sk-never-shown"));
    let (_, listed, _) = avatar(&data.0, &["providers"], "");
    let after = listed["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap()
        .clone();
    assert_eq!(after["keySet"], true);
    assert!(!listed.to_string().contains("sk-never-shown"));

    let (ok, cleared, _) = avatar(&data.0, &["key", "clear", id], "");
    assert!(ok);
    assert_eq!(cleared["cleared"], true);
    let (_, listed, _) = avatar(&data.0, &["providers"], "");
    assert!(
        listed["providers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["keySet"] == false)
    );
}

#[test]
fn a_key_is_stored_only_for_a_provider_that_uses_one() {
    let data = Scratch::new();
    let (_, listed, _) = avatar(&data.0, &["providers"], "");
    let local = listed["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["needsKey"] == false)
        .unwrap()
        .clone();
    let (ok, _, stderr) = avatar(
        &data.0,
        &["key", "set", local["id"].as_str().unwrap()],
        "sk-x",
    );
    assert!(!ok);
    assert!(stderr.contains("does not use a key"), "{stderr}");
    let (ok, _, stderr) = avatar(&data.0, &["key", "set", "no-such-provider"], "sk-x");
    assert!(!ok);
    assert!(stderr.contains("no-such-provider"), "{stderr}");
}

#[test]
fn check_measures_an_edited_frame_against_its_source_and_names_what_is_wrong() {
    let data = Scratch::new();
    let source = data.0.join("source.png");
    let good = data.0.join("good.png");
    let same = data.0.join("same.png");
    let base = |x: u32, y: u32| [(x % 200) as u8, (y % 200) as u8, 90, 255];
    fs::write(&source, rgba_png(128, 128, base)).unwrap();
    fs::write(&same, rgba_png(128, 128, base)).unwrap();
    fs::write(
        &good,
        rgba_png(128, 128, |x, y| {
            if (48..80).contains(&x) && (80..96).contains(&y) {
                [250, 20, 20, 255]
            } else {
                base(x, y)
            }
        }),
    )
    .unwrap();
    let source = source.to_str().unwrap();

    let (ok, report, stderr) = avatar(
        &data.0,
        &[
            "check",
            "--frame",
            "A",
            "--source",
            source,
            "--image",
            good.to_str().unwrap(),
            "--region",
            "40,72,48,32",
        ],
        "",
    );
    assert!(ok, "{stderr}");
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["changedBox"]["x"], 48);

    let (ok, report, _) = avatar(
        &data.0,
        &[
            "check",
            "--frame",
            "A",
            "--source",
            source,
            "--image",
            same.to_str().unwrap(),
        ],
        "",
    );
    assert!(ok);
    assert_eq!(report["ok"], false);
    assert_eq!(report["problems"][0], "unchanged");

    let (ok, _, stderr) = avatar(
        &data.0,
        &[
            "check", "--frame", "A", "--source", source, "--image", source, "--region", "1,2,3",
        ],
        "",
    );
    assert!(!ok);
    assert!(stderr.contains("--region"), "{stderr}");
}

#[test]
fn frame_refuses_what_it_cannot_do_before_it_calls_anything() {
    let data = Scratch::new();
    let source = data.0.join("source.png");
    fs::write(&source, rgba_png(64, 64, |_, _| [1, 2, 3, 255])).unwrap();
    let source = source.to_str().unwrap();
    let out = data.0.join("out.png");
    let out = out.to_str().unwrap();
    for (args, expect) in [
        (vec!["--provider", "nope", "--frame", "A"], "nope"),
        (
            vec!["--provider", "qwen-image-2.1-comfyui", "--frame", "zzz"],
            "zzz",
        ),
        (
            vec!["--provider", "openai-compatible-image-edit", "--frame", "A"],
            "address",
        ),
        (
            vec![
                "--provider",
                "openai-compatible-image-edit",
                "--frame",
                "A",
                "--endpoint",
                "https://example.test/v1",
                "--model",
                "m",
            ],
            "key",
        ),
    ] {
        let mut all = vec!["frame"];
        all.extend(args);
        all.extend(["--source", source, "--out", out]);
        let (ok, _, stderr) = avatar(&data.0, &all, "");
        assert!(!ok);
        assert!(stderr.contains(expect), "{expect}: {stderr}");
    }
    let (ok, _, stderr) = avatar(
        &data.0,
        &[
            "frame",
            "--provider",
            "qwen-image-2.1-comfyui",
            "--frame",
            "A",
            "--source",
            source,
            "--out",
            source,
        ],
        "",
    );
    assert!(!ok);
    assert!(stderr.contains("--out must not be the source"), "{stderr}");
}

/// A ComfyUI stand-in on the loopback: it answers every request by its path, and returns its
/// address.
fn comfy_stand_in(picture: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = Vec::new();
            let mut chunk = [0u8; 2048];
            // Read the head, then the body the head announces, so curl is never cut off.
            let (path, body_at, length) = loop {
                let read = stream.read(&mut chunk).unwrap_or(0);
                request.extend_from_slice(&chunk[..read]);
                let head_end = request.windows(4).position(|w| w == b"\r\n\r\n");
                if read == 0 || head_end.is_some() {
                    let head = String::from_utf8_lossy(&request).to_lowercase();
                    let path = head.split(' ').nth(1).unwrap_or("").to_owned();
                    let length = head
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    break (path, head_end.map_or(0, |at| at + 4), length);
                }
            };
            while request.len() < body_at + length {
                let read = stream.read(&mut chunk).unwrap_or(0);
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
            }
            let body = match path.split('?').next().unwrap_or("") {
                "/upload/image" => br#"{"name":"stored.png"}"#.to_vec(),
                "/prompt" => br#"{"prompt_id":"j"}"#.to_vec(),
                "/history/j" => br#"{"j":{"status":{"status_str":"success","completed":true},"outputs":{"461":{"images":[{"filename":"o.png"}]}}}}"#.to_vec(),
                _ => picture.clone(),
            };
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    address
}

#[test]
fn frame_makes_one_picture_with_the_provider_the_person_chose_and_writes_it() {
    let data = Scratch::new();
    let source = data.0.join("source.png");
    let made = rgba_png(64, 32, |x, _| [x as u8, 9, 9, 255]);
    fs::write(&source, rgba_png(64, 32, |_, _| [1, 2, 3, 255])).unwrap();
    let endpoint = comfy_stand_in(made.clone());
    let out = data.0.join("frames").join("A.png");
    fs::create_dir_all(out.parent().unwrap()).unwrap();
    let (ok, result, stderr) = avatar(
        &data.0,
        &[
            "frame",
            "--provider",
            "qwen-image-2.1-comfyui",
            "--frame",
            "A",
            "--source",
            source.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--endpoint",
            &endpoint,
            "--seed",
            "5",
            "--extra",
            "Keep the head still.",
        ],
        "",
    );
    assert!(ok, "{stderr}");
    assert_eq!(fs::read(&out).unwrap(), made);
    assert_eq!(result["frame"], "A");
    assert_eq!(result["seed"], 5);
    assert_eq!(
        (result["width"].clone(), result["height"].clone()),
        (64.into(), 32.into())
    );
}
