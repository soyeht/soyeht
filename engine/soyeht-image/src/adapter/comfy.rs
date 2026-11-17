//! A ComfyUI server: upload the picture, queue the entry's graph with this edit's values, wait
//! for the job and fetch the saved picture. The graph and the places the values go are the
//! entry's data (`Recipe`); nothing here knows which model the graph runs.

use super::{Edit, Job};
use crate::{
    catalogue::{Choice, Slot},
    error::{Error, Result},
    exchange::{self, invalid},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_platform::secret::Secret;
use soyeht_process::curl::Request;
use std::{
    fs,
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_secs(2);
/// The longest one request of the exchange may take.
const STEP: Duration = Duration::from_secs(60);
const CLIENT: &str = "soyeht-avatar";
/// Characters of the picture's digest in the name it gets on the server.
const NAME_DIGEST: usize = 12;

pub(super) fn edit(job: &Job<'_>) -> Result<Vec<u8>> {
    let Job {
        choice,
        key,
        edit,
        timeout,
    } = *job;
    let recipe = choice
        .descriptor
        .recipe
        .as_ref()
        .ok_or(Error::Data("a ComfyUI entry has no recipe"))?;
    let deadline = Instant::now() + timeout;
    let server = Server { choice, key };
    let name = server.upload(edit, deadline)?;
    let mut graph = recipe.graph.clone();
    let bindings = &recipe.bindings;
    place(&mut graph, &bindings.image, json!(name))?;
    place(&mut graph, &bindings.prompt, json!(edit.prompt))?;
    place(&mut graph, &bindings.seed, json!(edit.seed.unwrap_or(1)))?;
    place(&mut graph, &bindings.width, json!(edit.width))?;
    place(&mut graph, &bindings.height, json!(edit.height))?;
    let job = server.queue(&graph, deadline)?;
    let file = server.wait(&job, &bindings.output, deadline)?;
    server.view(&file, deadline)
}

/// Sets one input of one node of the graph.
fn place(graph: &mut Value, slot: &Slot, value: Value) -> Result<()> {
    let input = graph
        .get_mut(&slot.0)
        .and_then(|node| node.get_mut("inputs"))
        .and_then(|inputs| inputs.get_mut(&slot.1))
        .ok_or(Error::Data(
            "a binding names an input the graph does not have",
        ))?;
    *input = value;
    Ok(())
}

struct Server<'a> {
    choice: &'a Choice,
    key: Option<&'a Secret>,
}

impl Server<'_> {
    fn request(&self, segments: &[&str], query: &[(&str, &str)]) -> Result<Request> {
        let url = self.choice.url(segments, query)?;
        let request = exchange::request(&url, self.choice.https_only())?;
        match self.key {
            Some(key) => request.bearer(key).map_err(invalid),
            None => Ok(request),
        }
    }

    /// One request's time: at most `STEP`, and never past the whole job's deadline.
    fn step(deadline: Instant) -> Result<Duration> {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Error::TimedOut);
        }
        Ok(left.min(STEP))
    }

    fn upload(&self, edit: &Edit<'_>, deadline: Instant) -> Result<String> {
        let digest = format!("{:x}", Sha256::digest(fs::read(edit.source)?));
        let name = format!("soyeht-{}.png", &digest[..NAME_DIGEST]);
        let request = self
            .request(&["upload", "image"], &[])?
            .form_file("image", edit.source, &name, "image/png")
            .and_then(|request| request.form_text("overwrite", "true"))
            .map_err(invalid)?;
        let answer = exchange::send_json(request, self.key, Self::step(deadline)?)?;
        text(&answer, "name").ok_or(Error::BadResponse("no name for the uploaded picture"))
    }

    fn queue(&self, graph: &Value, deadline: Instant) -> Result<String> {
        let body = json!({"prompt": graph, "client_id": CLIENT}).to_string();
        let request = self
            .request(&["prompt"], &[])?
            .json_body(&body)
            .map_err(invalid)?;
        let answer = exchange::send_json(request, self.key, Self::step(deadline)?)?;
        text(&answer, "prompt_id").ok_or(Error::BadResponse("no job id"))
    }

    /// Waits for the job and returns the saved picture's `(filename, subfolder, type)`.
    fn wait(&self, job: &str, output: &str, deadline: Instant) -> Result<[String; 3]> {
        loop {
            let request = self.request(&["history", job], &[])?;
            let history = exchange::send_json(request, self.key, Self::step(deadline)?)?;
            if let Some(entry) = history.get(job) {
                let status = entry.pointer("/status/status_str").and_then(Value::as_str);
                if status == Some("error") {
                    return Err(Error::JobFailed);
                }
                if entry.pointer("/status/completed") == Some(&Value::Bool(true)) {
                    return saved(entry, output);
                }
            }
            if Instant::now() + POLL >= deadline {
                return Err(Error::TimedOut);
            }
            std::thread::sleep(POLL);
        }
    }

    fn view(&self, file: &[String; 3], deadline: Instant) -> Result<Vec<u8>> {
        let query = [
            ("filename", file[0].as_str()),
            ("subfolder", file[1].as_str()),
            ("type", file[2].as_str()),
        ];
        let request = self.request(&["view"], &query)?;
        exchange::send(request, self.key, Self::step(deadline)?)
    }
}

fn saved(entry: &Value, output: &str) -> Result<[String; 3]> {
    let image = entry
        .pointer(&format!("/outputs/{output}/images/0"))
        .ok_or(Error::BadResponse("the job saved no picture"))?;
    let filename =
        text(image, "filename").ok_or(Error::BadResponse("the saved picture has no name"))?;
    Ok([
        filename,
        text(image, "subfolder").unwrap_or_default(),
        text(image, "type").unwrap_or_else(|| "output".to_owned()),
    ])
}

/// The string at `key` of a JSON object.
fn text(object: &Value, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_owned)
}
