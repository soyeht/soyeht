//! A ComfyUI server: upload the picture, queue the entry's graph with this edit's values, wait
//! for the job and fetch the saved picture. The graph and the places the values go are the
//! entry's data (`Recipe`); nothing here knows which model the graph runs.

use super::{Edit, Job};
use crate::{
    catalogue::{Choice, Slot},
    curl::Request,
    error::{Error, Result},
    vault::Secret,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    time::{Duration, Instant},
};
use url::form_urlencoded;

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
    place(&mut graph, &bindings.seed, json!(edit.seed))?;
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
    fn request(&self, path: &str) -> Result<Request> {
        let request = Request::new(&self.choice.url(path), self.choice.https_only())?;
        match self.key {
            Some(key) => request.bearer(key),
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
        let answer = self
            .request("upload/image")?
            .png_field("image", edit.source, &name)?
            .field("overwrite", "true")?
            .send(Self::step(deadline)?)?
            .json()?;
        text(&answer, "name").ok_or(Error::BadResponse("no name for the uploaded picture"))
    }

    fn queue(&self, graph: &Value, deadline: Instant) -> Result<String> {
        let answer = self
            .request("prompt")?
            .json(&json!({"prompt": graph, "client_id": CLIENT}))?
            .send(Self::step(deadline)?)?
            .json()?;
        text(&answer, "prompt_id").ok_or(Error::BadResponse("no job id"))
    }

    /// Waits for the job and returns the saved picture's `(filename, subfolder, type)`.
    fn wait(&self, job: &str, output: &str, deadline: Instant) -> Result<[String; 3]> {
        loop {
            let history = self
                .request(&format!("history/{job}"))?
                .send(Self::step(deadline)?)?
                .json()?;
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
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("filename", &file[0])
            .append_pair("subfolder", &file[1])
            .append_pair("type", &file[2])
            .finish();
        self.request(&format!("view?{query}"))?
            .send(Self::step(deadline)?)?
            .success()
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
