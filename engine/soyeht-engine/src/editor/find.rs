//! `file.find` and `file.replace`: in-file search over the whole document.
//!
//! A client holds one page of a large file, so the engine counts matches,
//! picks the next or previous one and builds replacement edits against the
//! full text. Replacements go through the durable write path as one undo
//! transaction, like any other accepted edit.

use super::persistence;
use crate::{core::Core, errors, errors::Result};
use serde_json::Value;
use soyeht_editor::{ContentMatcher, FindDirection, FindRequest, MatchOptions};
use soyeht_protocol::{
    FileFindDirection, FileFindMatch, FileFindParams, FileFindQuery, FileFindResult,
    FileReplaceParams, FileReplaceResult,
};
use soyeht_store::editor::{EditorFile, document};

impl Core {
    pub fn file_find(&self, params: &Value) -> Result<Value> {
        let request: FileFindParams = persistence::parameters(params, "find")?;
        let base = persistence::revision(&request.revision)?;
        let (file, text, matcher) = self.find_target(&request.file_id, base, &request.query)?;
        let first = request.visible_from_line.unwrap_or(0);
        let count = request
            .visible_line_count
            .unwrap_or(0)
            .min(u32::try_from(soyeht_protocol::limits::EDITOR_VIEWPORT_LINES).unwrap_or(u32::MAX));
        let outcome = soyeht_editor::find(
            text,
            &FindRequest {
                matcher: &matcher,
                from: request.from,
                direction: match request.direction {
                    FileFindDirection::Next => FindDirection::Next,
                    FileFindDirection::Previous => FindDirection::Previous,
                },
                window: first..first.saturating_add(count),
                visible_limit: soyeht_protocol::limits::EDITOR_SEARCH_RESULTS,
            },
        );
        serde_json::to_value(FileFindResult {
            revision: file.revision.to_string(),
            total: u32::try_from(outcome.total).unwrap_or(u32::MAX),
            current: outcome.current.map(|found| FileFindMatch {
                index: u32::try_from(found.index).unwrap_or(u32::MAX),
                range: found.range,
            }),
            visible: outcome.visible,
        })
        .map_err(|_| errors::internal("File find result serialization failed"))
    }

    pub fn file_replace(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: FileReplaceParams = persistence::parameters(params, "replace")?;
        let base = persistence::revision(&request.base_revision)?;
        let author = self.file_author(connection)?;
        let (file, text, matcher) = self.find_target(&request.file_id, base, &request.query)?;
        let (edits, replaced) = match &request.range {
            Some(range) => {
                let edit = soyeht_editor::replace_one(text, &matcher, range, &request.replacement);
                let count = usize::from(edit.is_some());
                (edit.into_iter().collect(), count)
            }
            None => soyeht_editor::replace_all(text, &matcher, &request.replacement)?,
        };
        if edits.is_empty() {
            let result = replace_result(FileReplaceResult {
                file: document(file.clone()).map_err(errors::storage)?,
                replaced: 0,
                changed_lines: Vec::new(),
                cursor: None,
            })?;
            return Ok(self
                .mutate(connection, "file.replace", params, |_| {
                    Ok((result, Vec::new()))
                })?
                .result);
        }
        let prepared = self.editor.prepare_command_edit(
            &request.file_id,
            base,
            &edits,
            author,
            self.now_ms(),
        )?;
        let result = replace_result(FileReplaceResult {
            file: document(prepared.file.clone()).map_err(errors::storage)?,
            replaced: u32::try_from(replaced).unwrap_or(u32::MAX),
            changed_lines: prepared.changed_lines.clone(),
            cursor: prepared.cursor.clone(),
        })?;
        self.commit_prepared_edit(connection, "file.replace", params, prepared, result)
    }
}

impl Core {
    /// The open file at `revision`, its current text and the compiled query.
    fn find_target(
        &self,
        file_id: &str,
        revision: u64,
        query: &FileFindQuery,
    ) -> Result<(&EditorFile, &str, ContentMatcher)> {
        let file = self.editor.stored(file_id)?;
        if file.revision != revision {
            return Err(errors::stale_version("File revision changed"));
        }
        let matcher = ContentMatcher::with_options(
            &query.text,
            MatchOptions {
                case_sensitive: query.case_sensitive,
                whole_word: query.whole_word,
                regex: query.regex,
            },
        )?;
        let text = file.draft_text.as_deref().unwrap_or(&file.disk_text);
        Ok((file, text, matcher))
    }
}

fn replace_result(result: FileReplaceResult) -> Result<Value> {
    serde_json::to_value(result)
        .map_err(|_| errors::internal("File replace result serialization failed"))
}

#[cfg(test)]
mod tests {
    use super::super::test_support::Fixture;
    use crate::ports::testing::FixedClock;
    use serde_json::{Value, json};
    use std::sync::Arc;

    fn query(text: &str, regex: bool) -> Value {
        json!({"text":text,"caseSensitive":true,"wholeWord":false,"regex":regex})
    }

    fn find(fixture: &mut Fixture, query: Value, line: u32, direction: &str) -> Value {
        let revision = fixture.revision();
        let file_id = fixture.file_id.clone();
        fixture.call(
            "file.find",
            json!({
                "fileId":file_id,"revision":revision,"query":query,
                "from":{"line":line,"column":0},"direction":direction,
                "visibleFromLine":0,"visibleLineCount":200
            }),
        )
    }

    fn replace(fixture: &mut Fixture, query: Value, text: &str, range: Option<Value>) -> Value {
        let revision = fixture.revision();
        let file_id = fixture.file_id.clone();
        let mut params = json!({
            "fileId":file_id,"baseRevision":revision,"query":query,"replacement":text,
            "idempotencyKey":uuid::Uuid::new_v4().to_string()
        });
        if let Some(range) = range {
            params["range"] = range;
        }
        fixture.call("file.replace", params)
    }

    fn rejected(fixture: &mut Fixture, method: &str, params: Value) -> bool {
        let request = json!({"jsonrpc":"2.0","id":90,"method":method,"params":params});
        fixture.core.dispatch(&fixture.connection, request).is_err()
    }

    #[tokio::test]
    async fn find_counts_and_steps_through_matches_outside_the_loaded_page() {
        let text = (0..5_000)
            .map(|line| match line {
                10 | 2_500 => format!("needle {line}\n"),
                4_900 => "x needle\n".to_owned(),
                _ => format!("line {line}\n"),
            })
            .collect::<String>();
        let mut fixture = Fixture::open(&text).await;

        let found = find(&mut fixture, query("needle", false), 2_000, "next");
        assert_eq!(found["total"], 3);
        assert_eq!(found["current"]["index"], 1);
        assert_eq!(
            found["current"]["range"]["start"],
            json!({"line":2500,"column":0})
        );
        assert_eq!(found["visible"].as_array().unwrap().len(), 1);
        let previous = find(&mut fixture, query("needle", false), 10, "previous");
        assert_eq!(
            previous["current"]["range"]["start"],
            json!({"line":4900,"column":2})
        );
        let pattern = find(&mut fixture, query(r"^needle \d+$", true), 0, "next");
        assert_eq!(pattern["total"], 2);

        let file_id = fixture.file_id.clone();
        let from = json!({"line":0,"column":0});
        let stale = json!({"fileId":file_id,"revision":"999","query":query("needle", false),
            "from":from,"direction":"next"});
        assert!(rejected(&mut fixture, "file.find", stale));
        let invalid = json!({"fileId":file_id,"revision":"1","query":query("(", true),
            "from":from,"direction":"next"});
        assert!(rejected(&mut fixture, "file.find", invalid));
    }

    #[tokio::test]
    async fn replace_checks_the_match_and_replace_all_undoes_as_one_step() {
        let mut fixture = Fixture::open("let a = 1;\nlet b = a;\n").await;
        let stale_range = json!({"start":{"line":0,"column":0},"end":{"line":0,"column":2}});
        let refused = replace(&mut fixture, query("a", false), "z", Some(stale_range));
        assert_eq!(refused["replaced"], 0);
        assert_eq!(fixture.text(), "let a = 1;\nlet b = a;\n");

        let range = json!({"start":{"line":1,"column":8},"end":{"line":1,"column":9}});
        let single = replace(&mut fixture, query("a", false), "value", Some(range));
        assert_eq!(single["replaced"], 1);
        assert_eq!(single["cursor"], json!({"line":1,"column":13}));
        assert_eq!(fixture.text(), "let a = 1;\nlet b = value;\n");

        // Typing just before Replace All stays a separate undo step.
        fixture.type_at(5_000, 0, 0, "// ");
        fixture.core.clock = Arc::new(FixedClock(5_050));
        let all = replace(&mut fixture, query(r"let (\w)", true), "var ${1}1", None);
        assert_eq!(all["replaced"], 2);
        assert_eq!(fixture.text(), "// var a1 = 1;\nvar b1 = value;\n");
        assert_eq!(fixture.replay("file.undo", "undo-all")["applied"], true);
        assert_eq!(fixture.text(), "// let a = 1;\nlet b = value;\n");
        assert_eq!(fixture.replay("file.redo", "redo-all")["applied"], true);
        assert_eq!(fixture.text(), "// var a1 = 1;\nvar b1 = value;\n");
        assert_eq!(fixture.replay("file.undo", "undo-all-2")["applied"], true);
        assert_eq!(fixture.replay("file.undo", "undo-typing")["applied"], true);
        assert_eq!(fixture.text(), "let a = 1;\nlet b = value;\n");
    }
}
