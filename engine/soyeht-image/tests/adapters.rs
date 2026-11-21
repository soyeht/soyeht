#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_image::{
    Choice, Descriptor, Edit, Error, Secret, Vault, edit, find,
    testing::{Received, Scratch, Server, rgba_png},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(30);
const PROVIDER: &str = "local-openai";

fn picture() -> Vec<u8> {
    rgba_png(64, 32, |x, y| [x as u8, y as u8, 7, 255]).unwrap()
}

/// A source file and the edit that names it.
struct Job {
    scratch: Scratch,
    source: PathBuf,
}

impl Job {
    fn new() -> Self {
        let scratch = Scratch::new("image-adapters");
        let source = scratch.path("source.png");
        std::fs::write(&source, picture()).unwrap();
        Self { scratch, source }
    }

    fn edit(&self) -> Edit<'_> {
        Edit {
            source: &self.source,
            prompt: "Change only \"the mouth\".\nKeep the rest.",
            width: 64,
            height: 32,
            seed: Some(77),
        }
    }
}

/// A local entry that speaks the OpenAI request, as a server on the person's own network does.
fn local_openai() -> Descriptor {
    serde_json::from_value(json!({
        "id": PROVIDER, "name": "n", "summary": "s",
        "locality": "local", "adapter": "openai-images",
        "needsKey": true, "timeoutSeconds": 30, "sizeMultiple": 1,
        "license": {"name": "terms"}, "sends": ["picture", "text"]
    }))
    .unwrap()
}

fn chosen(descriptor: Descriptor, address: &str, model: Option<&str>) -> Choice {
    descriptor.choose(Some(address), model).unwrap()
}

/// The key `text`, stored for the address of `choice` and read back.
fn key(job: &Job, choice: &Choice, text: &str) -> Secret {
    let vault = Vault::new(&job.scratch.0);
    vault.set(PROVIDER, &choice.key_host(), text).unwrap();
    vault.key_for(PROVIDER, &choice.key_host()).unwrap()
}

/// One edit through `descriptor` at `server`, with a key stored for it.
fn attempt(job: &Job, descriptor: Descriptor, server: &Server) -> Result<Vec<u8>, Error> {
    let choice = chosen(descriptor, &server.url, Some("m"));
    let secret = key(job, &choice, "sk-test-key");
    edit(&choice, Some(&secret), &job.edit(), TIMEOUT)
}

fn encoded_answer() -> Vec<u8> {
    json!({"data": [{"b64_json": STANDARD.encode(picture())}]})
        .to_string()
        .into_bytes()
}

fn answering_picture() -> Server {
    Server::start(|_| (200, encoded_answer())).unwrap()
}

#[test]
fn the_openai_request_carries_the_picture_the_instruction_the_model_and_the_key() {
    let job = Job::new();
    let server = answering_picture();
    let choice = chosen(local_openai(), &server.url, Some("some-model"));
    let secret = key(&job, &choice, "sk-test-key");
    let made = edit(&choice, Some(&secret), &job.edit(), TIMEOUT).unwrap();
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
    let signature = [0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    assert!(sent.body.windows(8).any(|window| window == signature));
}

// Windows does not allow a quote or a backslash in a file name, so the file cannot exist there.
#[cfg(unix)]
#[test]
fn a_file_name_with_a_quote_a_semicolon_and_a_backslash_is_uploaded_whole() {
    let job = Job::new();
    let odd = job.scratch.path("we\"ird;na\\me.png");
    std::fs::write(&odd, picture()).unwrap();
    let server = answering_picture();
    let choice = chosen(local_openai(), &server.url, Some("m"));
    let secret = key(&job, &choice, "sk-test-key");
    let mut request = job.edit();
    request.source = &odd;
    edit(&choice, Some(&secret), &request, TIMEOUT).unwrap();
    let signature = [0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    assert!(server.received()[0].body.windows(8).any(|w| w == signature));
}

#[test]
fn a_provider_that_needs_a_key_is_not_called_without_one() {
    let job = Job::new();
    let server = answering_picture();
    let choice = chosen(local_openai(), &server.url, Some("m"));
    let result = edit(&choice, None, &job.edit(), TIMEOUT);
    assert!(matches!(result, Err(Error::MissingKey)));
    assert!(server.received().is_empty());
}

#[test]
fn a_key_is_never_sent_over_plain_http_to_another_computer() {
    let job = Job::new();
    let choice = chosen(local_openai(), "http://192.0.2.7:9000", Some("m"));
    let secret = key(&job, &choice, "sk-test-key");
    let result = edit(&choice, Some(&secret), &job.edit(), TIMEOUT);
    assert!(
        matches!(result, Err(Error::InvalidEndpoint(_))),
        "{result:?}"
    );
}

#[test]
fn a_refused_key_a_rate_limit_and_an_explained_refusal_are_told_apart() {
    let job = Job::new();
    for (status, body, check) in [
        (401, "{}", "unauthorized"),
        (429, "{}", "rate"),
        (
            400,
            r#"{"error":{"message":"Prompt was rejected, key sk-test-key"}}"#,
            "reason",
        ),
        (500, "oops", "bare"),
    ] {
        let server = Server::start(move |_| (status, body.as_bytes().to_vec())).unwrap();
        let choice = chosen(local_openai(), &server.url, Some("m"));
        let secret = key(&job, &choice, "sk-test-key");
        let error = edit(&choice, Some(&secret), &job.edit(), TIMEOUT).unwrap_err();
        assert!(!error.to_string().contains("sk-test-key"), "{error}");
        match check {
            "unauthorized" => assert!(matches!(error, Error::Unauthorized)),
            "rate" => assert!(matches!(error, Error::RateLimited)),
            "reason" => assert!(
                matches!(&error, Error::Provider { status: 400, reason: Some(reason) }
                if reason == "Prompt was rejected, key [key]")
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
fn a_redirect_is_not_followed_and_the_key_goes_to_one_request_only() {
    let job = Job::new();
    let server = Server::start(|_| (302, Vec::new())).unwrap();
    let result = attempt(&job, local_openai(), &server);
    assert!(
        matches!(result, Err(Error::Provider { status: 302, .. })),
        "{result:?}"
    );
    assert_eq!(server.received().len(), 1);
}

#[test]
fn an_answer_that_is_not_a_png_is_refused() {
    let job = Job::new();
    let body = json!({"data": [{"b64_json": STANDARD.encode(b"just text")}]}).to_string();
    let server = Server::start(move |_| (200, body.clone().into_bytes())).unwrap();
    let result = attempt(&job, local_openai(), &server);
    assert!(matches!(result, Err(Error::BadResponse(_))));
}

#[test]
fn a_provider_that_needs_sizes_in_steps_refuses_a_picture_that_is_not() {
    let job = Job::new();
    let server = answering_picture();
    let mut descriptor = local_openai();
    descriptor.size_multiple = 48;
    let result = attempt(&job, descriptor, &server);
    assert!(matches!(
        result,
        Err(Error::SourceSize { multiple: 48, .. })
    ));
    assert!(server.received().is_empty());
}

/// A ComfyUI server that finishes the job after `pending` empty answers.
fn comfy(pending: usize) -> Server {
    let polls = AtomicUsize::new(0);
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
                if polls.fetch_add(1, Ordering::SeqCst) < pending {
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
    .unwrap()
}

fn qwen(server: &Server) -> Choice {
    find("qwen-image-2.1-comfyui")
        .unwrap()
        .choose(Some(&server.url), None)
        .unwrap()
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
    let made = edit(&qwen(&server), None, &job.edit(), TIMEOUT).unwrap();
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
    assert!(edit(&qwen(&server), None, &job.edit(), TIMEOUT).is_ok());
    let polls = server
        .received()
        .iter()
        .filter(|r| r.path.starts_with("/history/"))
        .count();
    assert_eq!(polls, 2);
}

#[test]
fn a_job_id_the_server_sends_cannot_change_the_path_it_is_asked_for() {
    let job = Job::new();
    let server = Server::start(|request| match request.path.as_str() {
        "/upload/image" => (200, br#"{"name":"stored.png"}"#.to_vec()),
        "/prompt" => (200, br#"{"prompt_id":"../upload/image?x=1#"}"#.to_vec()),
        _ => (200, br#"{}"#.to_vec()),
    })
    .unwrap();
    let result = edit(&qwen(&server), None, &job.edit(), Duration::from_secs(1));
    assert!(result.is_err());
    let paths: Vec<_> = server.received().iter().map(|r| r.path.clone()).collect();
    assert_eq!(
        paths[2], "/history/..%2Fupload%2Fimage%3Fx=1%23",
        "{paths:?}"
    );
}

#[test]
fn a_comfyui_job_that_does_not_finish_in_time_or_fails_is_reported() {
    let job = Job::new();
    let never = comfy(usize::MAX);
    let result = edit(&qwen(&never), None, &job.edit(), Duration::from_secs(1));
    assert!(matches!(result, Err(Error::TimedOut)), "{result:?}");

    let failing = Server::start(|request| match request.path.as_str() {
        "/upload/image" => (200, br#"{"name":"stored.png"}"#.to_vec()),
        "/prompt" => (200, br#"{"prompt_id":"job-1"}"#.to_vec()),
        _ => (
            200,
            br#"{"job-1":{"status":{"status_str":"error","completed":false}}}"#.to_vec(),
        ),
    })
    .unwrap();
    let result = edit(&qwen(&failing), None, &job.edit(), TIMEOUT);
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
    use std::sync::{Arc, atomic::AtomicBool};
    let job = Job::new();
    let leaked = Arc::new(AtomicBool::new(false));
    let server = {
        let leaked = leaked.clone();
        Server::start(move |_| {
            for entry in std::fs::read_dir("/proc").unwrap().flatten() {
                let line = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
                if line.windows(11).any(|window| window == b"sk-test-key") {
                    leaked.store(true, Ordering::SeqCst);
                }
            }
            (200, encoded_answer())
        })
        .unwrap()
    };
    let choice = chosen(local_openai(), &server.url, Some("m"));
    let secret = key(&job, &choice, "sk-test-key");
    edit(&choice, Some(&secret), &job.edit(), TIMEOUT).unwrap();
    assert!(!leaked.load(Ordering::SeqCst));
    assert_eq!(
        server.received()[0].headers["authorization"],
        "Bearer sk-test-key"
    );
}

/// A header value cannot become two headers on the wire: the attempt is refused before anything
/// is sent, and an ordinary value arrives as exactly one header.
#[test]
fn a_header_value_arrives_as_one_header_and_an_injected_one_is_never_sent() {
    use soyeht_process::curl::{Request, system_program};
    let server = Server::start(|_| (200, b"{}".to_vec())).unwrap();
    assert!(
        Request::new(&server.url)
            .unwrap()
            .header("X-A", "a\nX-Injected: yes")
            .is_err()
    );
    let response = Request::new(&server.url)
        .unwrap()
        .header("X-A", "a b")
        .unwrap()
        .send(&system_program(), TIMEOUT, 1 << 16)
        .unwrap();
    assert_eq!(response.status, 200);
    let sent = server.received();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].headers["x-a"], "a b");
    assert!(!sent[0].headers.contains_key("x-injected"));
}
