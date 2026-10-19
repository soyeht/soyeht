//! Opening a file shows its first page from the reply of `file.open`, and a folder that is not
//! a repository is not an error (the editor asks for `file.diff` after every `file.open`).
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::{Value, json};
use std::fs;
use support::Rig;

fn texts(page: &Value) -> Vec<String> {
    page["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line["text"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_folder_that_is_not_a_repository_has_a_diff_with_nothing_in_it() {
    let rig = Rig::new();
    let folder = rig.path.join("plain-folder");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("note.txt"), "one\ntwo\n").unwrap();
    let mut client = rig.client();
    let workspace = client.add_workspace(&folder, "plain");
    let opened = client.open_file(&workspace, "note.txt");
    let file = &opened["file"];

    let reply = client.request(
        "file.diff",
        json!({"fileId":file["fileId"],"revision":file["revision"]}),
    );

    assert!(reply.get("error").is_none(), "{reply}");
    assert_eq!(reply["result"]["hunks"], json!([]));
    assert_eq!(reply["result"]["blame"], json!([]));
}

/// A 10 MB file shows its first page from the reply of `file.open`, with no `file.read`, and
/// each later page is read from the document the engine holds, which follows the edits.
#[test]
fn a_large_file_shows_its_first_page_with_the_open_and_reads_the_rest_lazily() {
    let rig = Rig::new();
    let folder = rig.path.join("large-folder");
    fs::create_dir_all(&folder).unwrap();
    let text = (0..105_000)
        .map(|n| format!("line {n:06} {}\n", "x".repeat(86)))
        .collect::<String>();
    fs::write(folder.join("large.txt"), &text).unwrap();
    let mut client = rig.client();
    let workspace = client.add_workspace(&folder, "large");

    let opened = client.open_file(&workspace, "large.txt");
    let first = texts(&opened["viewport"]);
    assert!(first.len() >= 100, "the open carries a page of its own");
    assert!(first[0].starts_with("line 000000"));
    let file_id = opened["file"]["fileId"].clone();
    let revision = opened["file"]["revision"].clone();

    let far = client.call(
        "file.read",
        json!({"fileId":file_id,"revision":revision,"fromLine":100_000,"lineCount":400}),
    );
    assert_eq!(far["fromLine"], 100_000);
    assert!(texts(&far)[0].starts_with("line 100000"));
    let again = client.call(
        "file.read",
        json!({"fileId":file_id,"revision":revision,"fromLine":0,"lineCount":first.len()}),
    );
    assert_eq!(texts(&again), first);

    // An edit changes what the next page says.
    let written = client.call(
        "file.write",
        json!({
            "fileId":file_id,"baseRevision":revision,
            "author":{"actor":{"role":"human_local","client":"runtime-test"},"name":"you"},
            "edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":4}},"text":"LINE"}],
            "idempotencyKey":"large-edit"
        }),
    );
    let edited = client.call(
        "file.read",
        json!({"fileId":file_id,"revision":written["file"]["revision"],"fromLine":0,"lineCount":2}),
    );
    assert!(texts(&edited)[0].starts_with("LINE 000000"));
}
