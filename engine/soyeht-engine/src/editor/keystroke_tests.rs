//! Typing stays a quick, in-memory operation under the Core lock (ADR 0079):
//! the per-keystroke editor requests answer directly from `dispatch`, never
//! through `Outcome::Offload` or a background job, so they take no extra hop.

use super::test_fixture::EditorFixture;
use crate::dispatch::Outcome;
use serde_json::json;

#[tokio::test]
async fn keystrokes_answer_directly_under_the_lock_without_offloading() {
    let mut fixture = EditorFixture::new(&[("src/lib.rs", "fn main() {}\n")]).await;
    let file_id = fixture.open("src/lib.rs", false)["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    for (column, key) in "typed".chars().enumerate() {
        let revision = fixture.read(&file_id)["file"]["revision"]
            .as_str()
            .unwrap()
            .to_owned();
        let position = json!({"line":0,"column":column});
        let request = json!({
            "jsonrpc":"2.0","id":1000 + column,"method":"file.write",
            "params":{
                "fileId":file_id,"baseRevision":revision,
                "author":{"actor":{"role":"human_local","client":"editor-test"},"name":"you"},
                "edits":[{"range":{"start":position,"end":position},"text":key.to_string()}],
                "idempotencyKey":format!("keystroke-{column}")
            }
        });
        let outcome = fixture.core.dispatch(&fixture.connection, request).unwrap();
        assert!(
            matches!(outcome, Outcome::Reply(_)),
            "file.write must answer under the lock, not offload"
        );
        let read = json!({
            "jsonrpc":"2.0","id":2000 + column,"method":"file.read",
            "params":{"fileId":file_id,"fromLine":0,"lineCount":20}
        });
        assert!(matches!(
            fixture.core.dispatch(&fixture.connection, read).unwrap(),
            Outcome::Reply(_)
        ));
    }
    assert!(fixture.text(&file_id).starts_with("typedfn main() {}"));
    assert!(
        !fixture.core.background.has_work(),
        "typing must not start background jobs"
    );
}
