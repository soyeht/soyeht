//! One open editor file over `EditorFixture`, for request-level tests.

use crate::editor::test_fixture::EditorFixture;
use serde_json::{Value, json};
use std::ops::{Deref, DerefMut};

pub(super) struct Fixture {
    base: EditorFixture,
    pub file_id: String,
}

impl Deref for Fixture {
    type Target = EditorFixture;
    fn deref(&self) -> &EditorFixture {
        &self.base
    }
}

impl DerefMut for Fixture {
    fn deref_mut(&mut self) -> &mut EditorFixture {
        &mut self.base
    }
}

impl Fixture {
    pub async fn open(text: &str) -> Self {
        let mut base = EditorFixture::new(&[("main.rs", text)]).await;
        let file_id = base.open("main.rs", false)["file"]["fileId"]
            .as_str()
            .unwrap()
            .to_owned();
        Self { base, file_id }
    }

    pub fn read(&mut self) -> Value {
        let file_id = self.file_id.clone();
        self.base.read(&file_id)
    }

    pub fn revision(&mut self) -> String {
        self.read()["file"]["revision"].as_str().unwrap().to_owned()
    }

    pub fn text(&mut self) -> String {
        let file_id = self.file_id.clone();
        self.base.text(&file_id)
    }

    pub fn type_at(&mut self, now_ms: i64, line: u32, column: u32, text: &str) {
        self.base.at(now_ms);
        let file_id = self.file_id.clone();
        self.base.type_at(&file_id, line, column, text);
    }

    pub fn replay(&mut self, method: &str, key: &str) -> Value {
        let revision = self.revision();
        let file_id = self.file_id.clone();
        self.call(
            method,
            json!({"fileId":file_id,"baseRevision":revision,"idempotencyKey":key}),
        )
    }
}
