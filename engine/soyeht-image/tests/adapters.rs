#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_image::{Choice, Descriptor, Edit, Error, Vault, edit, find};
use std::time::Duration;
use support::{Received, Scratch, Server, rgba_png, write};

const TIMEOUT: Duration = Duration::from_secs(30);

fn picture() -> Vec<u8> {
    rgba_png(64, 32, |x, y| [x as u8, y as u8, 7, 255])
}

/// A source file and the edit that names it.
struct Job {
    scratch: Scratch,
    source: std::path::PathBuf,
}

impl Job {
    fn new() -> Self {
        let scratch = Scratch::new();
        let source = scratch.path("source.png");
        write(&source, &picture());
        Self { scratch, source }
    }

    fn edit(&self) -> Edit<'_> {
        Edit {
            source: &self.source,
            prompt: "Change only \"the mouth\".\nKeep the rest.",
            width: 64,
            height: 32,
            seed: 77,
        }
    }
}

/// A local entry that speaks the OpenAI request, as a server on the person's own network does.
fn local_openai() -> Descriptor {
    serde_json::from_value(json!({
        "id": "local-openai", "name": "n", "summary": "s",
        "locality": "local", "adapter": "openai-images",
        "needsKey": true, "timeoutSeconds": 30, "sizeMultiple": 1,
        "license": {"name": "terms"}, "sends": ["picture", "text"]
    }))
    .unwrap()
}

fn chosen(descriptor: Descriptor, server: &Server, model: Option<&str>) -> Choice {
    descriptor.choose(Some(&server.url), model).unwrap()
}

fn key(scratch: &Scratch, text: &str) -> soyeht_image::Secret {
    let vault = Vault::new(&scratch.0);
    vault.set("local-openai", text).unwrap();
    vault.read("local-openai").unwrap().unwrap()
}

fn encoded_answer() -> Vec<u8> {
    json!({"data": [{"b64_json": STANDARD.encode(picture())}]})
        .to_string()
        .into_bytes()
}

#[test]
fn the_openai_request_carries_the_picture_the_instruction_the_model_and_the_key() {
    let job = Job::new();
    let server = Server::start(|_| (200, encoded_answer()));
    let secret = key(&job.scratch, "sk-test-key");
    let made = edit(
        &chosen(local_openai(), &server, Some("some-model")),
        Some(&secret),
        &job.edit(),
        TIMEOUT,
    )
    .unwrap();
    assert_eq!(made, picture());
    let sent = &server.received()[0];
    assert_eq!(
        (sent.method.as_str(), sent.path.as_str()),
        ("POST", "/images/edits")
    );
    assert_eq!(sent.headers["authorization"], "Bearer sk-test-key");
    let body = sent.text();
    assert!(body.contains("name=\"model\"\r\n\r\nsome-model"));
    assert!(body.contains("name=\"size\"\r\n\r\n64x32"));
    assert!(body.contains("name=\"prompt\"\r\n\r\nChange only \"the mouth\".\nKeep the rest."));
    assert!(body.contains("name=\"image\"; filename=\"avatar.png\""));
    assert!(
        sent.body
            .windows(8)
            .any(|window| window == [0x89, b'P', b'N', b'G', 13, 10, 26, 10])
    );
}

#[test]
fn a_picture_given_as_an_address_is_fetched_without_the_key_and_only_over_https() {
    let job = Job::new();
    let server = Server::start(|_| {
        (
            200,
            json!({"data": [{"url": "http://example.test/p.png"}]})
                .to_string()
                .into_bytes(),
        )
    });
    let secret = key(&job.scratch, "sk-test-key");
    let refused = edit(
        &chosen(local_openai(), &server, Some("m")),
        Some(&secret),
        &job.edit(),
        TIMEOUT,
    );
    assert!(refused.is_err());
    assert_eq!(server.received().len(), 1, "the address was not fetched");
}

#[test]
fn a_provider_that_needs_a_key_is_not_called_without_one() {
    let job = Job::new();
    let server = Server::start(|_| (200, encoded_answer()));
    let result = edit(
        &chosen(local_openai(), &server, Some("m")),
        None,
        &job.edit(),
        TIMEOUT,
    );
    assert!(matches!(result, Err(Error::MissingKey)));
    assert!(server.received().is_empty());
}

#[test]
fn a_refused_key_a_rate_limit_and_an_explained_refusal_are_told_apart() {
    let job = Job::new();
    let secret = key(&job.scratch, "sk-test-key");
    for (status, body, check) in [
        (401, "{}", "unauthorized"),
        (429, "{}", "rate"),
        (
            400,
            r#"{"error":{"message":"Prompt was rejected"}}"#,
            "reason",
        ),
        (500, "oops", "bare"),
    ] {
        let server = Server::start(move |_| (status, body.as_bytes().to_vec()));
        let error = edit(
            &chosen(local_openai(), &server, Some("m")),
            Some(&secret),
            &job.edit(),
            TIMEOUT,
        )
        .unwrap_err();
        let text = error.to_string();
        assert!(!text.contains("sk-test-key"), "{text}");
        match check {
            "unauthorized" => assert!(matches!(error, Error::Unauthorized)),
            "rate" => assert!(matches!(error, Error::RateLimited)),
            "reason" => assert!(
                matches!(&error, Error::Provider { status: 400, reason: Some(reason) } if reason == "Prompt was rejected")
            ),
            _ => assert!(matches!(
                error,
                Error::Provider {
                    status: 500,
                    reason: None
                }
            )),
        }
    }
}

#[test]
fn an_answer_that_is_not_a_png_is_refused() {
    let job = Job::new();
    let secret = key(&job.scratch, "sk-test-key");
    let server = Server::start(|_| {
        (
            200,
            json!({"data": [{"b64_json": STANDARD.encode(b"just text")}]})
                .to_string()
                .into_bytes(),
        )
    });
    let result = edit(
        &chosen(local_openai(), &server, Some("m")),
        Some(&secret),
        &job.edit(),
        TIMEOUT,
    );
    assert!(matches!(result, Err(Error::BadResponse(_))));
}

#[test]
fn a_provider_that_needs_sizes_in_steps_refuses_a_picture_that_is_not() {
    let job = Job::new();
    let server = Server::start(|_| (200, encoded_answer()));
    let mut descriptor = local_openai();
    descriptor.size_multiple = 48;
    let secret = key(&job.scratch, "sk-test-key");
    let result = edit(
        &chosen(descriptor, &server, Some("m")),
        Some(&secret),
        &job.edit(),
        TIMEOUT,
    );
    assert!(matches!(
        result,
        Err(Error::SourceSize { multiple: 48, .. })
    ));
    assert!(server.received().is_empty());
}

/// A ComfyUI server that finishes the job after `pending` empty answers.
fn comfy(pending: usize) -> Server {
    let polls = std::sync::atomic::AtomicUsize::new(0);
    Server::start(
        move |request: &Received| match request.path.split('?').next().unwrap() {
            "/upload/image" => (
                200,
                json!({"name": "stored.png", "subfolder": "", "type": "input"})
                    .to_string()
                    .into_bytes(),
            ),
            "/prompt" => (200, json!({"prompt_id": "job-1"}).to_string().into_bytes()),
            "/history/job-1" => {
                if polls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < pending {
                    return (200, b"{}".to_vec());
                }
                let done = json!({"job-1": {
                    "status": {"status_str": "success", "completed": true},
                    "outputs": {"461": {"images": [{"filename": "out 1.png", "subfolder": "b", "type": "output"}]}}
                }});
                (200, done.to_string().into_bytes())
            }
            "/view" => (200, picture()),
            _ => (404, b"{}".to_vec()),
        },
    )
}

fn sent_graph(server: &Server) -> Value {
    let queued = server
        .received()
        .into_iter()
        .find(|request| request.path == "/prompt")
        .unwrap();
    serde_json::from_slice::<Value>(&queued.body).unwrap()["prompt"].clone()
}

#[test]
fn a_comfyui_entry_uploads_queues_its_graph_with_the_edits_values_and_fetches_the_result() {
    let job = Job::new();
    let server = comfy(0);
    let choice = find("qwen-image-2.1-comfyui")
        .unwrap()
        .choose(Some(&server.url), None)
        .unwrap();
    let made = edit(&choice, None, &job.edit(), TIMEOUT).unwrap();
    assert_eq!(made, picture());
    let paths: Vec<_> = server.received().iter().map(|r| r.path.clone()).collect();
    assert_eq!(paths[0], "/upload/image");
    assert_eq!(paths[1], "/prompt");
    assert!(
        paths[3].starts_with("/view?filename=out+1.png&subfolder=b&type=output"),
        "{paths:?}"
    );
    let upload = server.received()[0].text();
    assert!(upload.contains("name=\"overwrite\"\r\n\r\ntrue"));
    let graph = sent_graph(&server);
    assert_eq!(graph["470"]["inputs"]["image"], "stored.png");
    assert_eq!(
        graph["459:474"]["inputs"]["prompt"],
        "Change only \"the mouth\".\nKeep the rest."
    );
    assert_eq!(graph["459:458"]["inputs"]["seed"], 77);
    assert_eq!(graph["459:456"]["inputs"]["width"], 64);
    assert_eq!(graph["459:456"]["inputs"]["height"], 32);
    assert_eq!(
        graph["459:458"]["inputs"]["steps"], 25,
        "the rest of the graph is the entry's"
    );
}

#[test]
fn a_comfyui_job_that_is_still_running_is_waited_for() {
    let job = Job::new();
    let server = comfy(1);
    let choice = find("qwen-image-2.1-comfyui")
        .unwrap()
        .choose(Some(&server.url), None)
        .unwrap();
    assert!(edit(&choice, None, &job.edit(), TIMEOUT).is_ok());
    let polls = server
        .received()
        .iter()
        .filter(|r| r.path.starts_with("/history/"))
        .count();
    assert_eq!(polls, 2);
}

#[test]
fn a_comfyui_job_that_does_not_finish_in_time_or_fails_is_reported() {
    let job = Job::new();
    let never = comfy(usize::MAX);
    let choice = find("qwen-image-2.1-comfyui")
        .unwrap()
        .choose(Some(&never.url), None)
        .unwrap();
    let result = edit(&choice, None, &job.edit(), Duration::from_secs(1));
    assert!(matches!(result, Err(Error::TimedOut)), "{result:?}");

    let failing = Server::start(|request| match request.path.as_str() {
        "/upload/image" => (200, br#"{"name":"stored.png"}"#.to_vec()),
        "/prompt" => (200, br#"{"prompt_id":"job-1"}"#.to_vec()),
        _ => (
            200,
            br#"{"job-1":{"status":{"status_str":"error","completed":false}}}"#.to_vec(),
        ),
    });
    let choice = find("qwen-image-2.1-comfyui")
        .unwrap()
        .choose(Some(&failing.url), None)
        .unwrap();
    let result = edit(&choice, None, &job.edit(), TIMEOUT);
    assert!(matches!(result, Err(Error::JobFailed)), "{result:?}");
}

#[test]
fn a_server_that_is_not_there_is_unreachable() {
    let job = Job::new();
    let choice = find("qwen-image-2.1-comfyui")
        .unwrap()
        .choose(Some("http://127.0.0.1:1"), None)
        .unwrap();
    let result = edit(&choice, None, &job.edit(), Duration::from_secs(5));
    assert!(matches!(result, Err(Error::Unreachable)), "{result:?}");
}

/// While a request is in flight, no process of this computer has the key on its command line.
#[cfg(target_os = "linux")]
#[test]
fn the_key_is_never_on_a_command_line() {
    let job = Job::new();
    let leaked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server = {
        let leaked = leaked.clone();
        Server::start(move |_| {
            for entry in std::fs::read_dir("/proc").unwrap().flatten() {
                let line = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
                if line.windows(11).any(|window| window == b"sk-test-key") {
                    leaked.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
            (200, encoded_answer())
        })
    };
    let secret = key(&job.scratch, "sk-test-key");
    edit(
        &chosen(local_openai(), &server, Some("m")),
        Some(&secret),
        &job.edit(),
        TIMEOUT,
    )
    .unwrap();
    assert!(!leaked.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        server.received()[0].headers["authorization"],
        "Bearer sk-test-key"
    );
}
