//! Agent replies as the phone draws them: the blocks of the Markdown subset every Soyeht client shares
//! (ADR 0087). The rules are `soyeht_client::markdown`'s, which passes `contracts/vectors/markdown.json`;
//! this only gives its blocks to Kotlin, which draws them. A bare web address in the text is linked the
//! way the desktop links it (`contracts/vectors/web-links.json`).

use soyeht_client::markdown::{self, Block, Run};
use soyeht_protocol::client_policy;

/// A stretch of text in one style.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct MarkdownRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    /// An `http`, `https` or `mailto` address the text opens; no other address is ever kept.
    pub link: Option<String>,
}

/// The runs of one table cell.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct MarkdownCell {
    pub runs: Vec<MarkdownRun>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct MarkdownRow {
    pub cells: Vec<MarkdownCell>,
}

/// The blocks of one list item.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct MarkdownItem {
    pub blocks: Vec<MarkdownBlock>,
}

#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum MarkdownBlock {
    Paragraph {
        runs: Vec<MarkdownRun>,
    },
    Heading {
        level: u8,
        runs: Vec<MarkdownRun>,
    },
    /// `text` is what the Copy button puts on the clipboard: no fence, no language tag.
    Code {
        language: Option<String>,
        text: String,
    },
    Quote {
        blocks: Vec<MarkdownBlock>,
    },
    /// Named for what it holds: a variant called `List` would shadow Kotlin's own in the bindings.
    ItemList {
        /// The first number of an ordered list; none for a bullet list.
        start: Option<u64>,
        items: Vec<MarkdownItem>,
    },
    Table {
        header: MarkdownRow,
        rows: Vec<MarkdownRow>,
    },
    Rule,
}

fn runs(runs: Vec<Run>) -> Vec<MarkdownRun> {
    runs.into_iter()
        .map(|run| MarkdownRun {
            text: run.text,
            bold: run.style.bold,
            italic: run.style.italic,
            strike: run.style.strike,
            code: run.style.code,
            link: run.style.link,
        })
        .collect()
}

fn row(cells: Vec<Vec<Run>>) -> MarkdownRow {
    MarkdownRow {
        cells: cells
            .into_iter()
            .map(|cell| MarkdownCell { runs: runs(cell) })
            .collect(),
    }
}

/// The parser's blocks as the views get them, nothing added and nothing left out.
fn project(blocks: Vec<Block>) -> Vec<MarkdownBlock> {
    blocks
        .into_iter()
        .map(|block| match block {
            Block::Paragraph(content) => MarkdownBlock::Paragraph {
                runs: runs(content),
            },
            Block::Heading {
                level,
                runs: content,
            } => MarkdownBlock::Heading {
                level,
                runs: runs(content),
            },
            Block::Code { language, text } => MarkdownBlock::Code { language, text },
            Block::Quote(inner) => MarkdownBlock::Quote {
                blocks: project(inner),
            },
            Block::List { start, items } => MarkdownBlock::ItemList {
                start,
                items: items
                    .into_iter()
                    .map(|item| MarkdownItem {
                        blocks: project(item),
                    })
                    .collect(),
            },
            Block::Table { header, rows } => MarkdownBlock::Table {
                header: row(header),
                rows: rows.into_iter().map(row).collect(),
            },
            Block::Rule => MarkdownBlock::Rule,
        })
        .collect()
}

/// The blocks of an agent's reply. `streaming` is true while the reply is still arriving: the spans it
/// has left open are closed, so a bold word shows its style as it is typed.
#[uniffi::export]
pub fn markdown_blocks(text: String, streaming: bool) -> Vec<MarkdownBlock> {
    project(markdown::link_blocks(markdown::parse(&text, streaming)))
}

/// What a code block's Copy button says and how long it shows that it copied.
#[derive(uniffi::Record)]
pub struct CodeCopy {
    pub label: String,
    pub copied: String,
    pub feedback_millis: u64,
}

#[uniffi::export]
pub fn code_copy() -> CodeCopy {
    CodeCopy {
        label: client_policy::COPY_CODE.to_owned(),
        copied: client_policy::COPIED_CODE.to_owned(),
        feedback_millis: client_policy::COPIED_FEEDBACK_MS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// The Markdown rules every client shares; the Mac, the iPhone and the desktop pass the same file.
    const VECTORS: &str = include_str!("../../../../contracts/vectors/markdown.json");

    fn run(run: &MarkdownRun) -> Value {
        let mut value = json!({"text": run.text});
        for (key, on) in [
            ("bold", run.bold),
            ("italic", run.italic),
            ("strike", run.strike),
            ("code", run.code),
        ] {
            if on {
                value[key] = json!(true);
            }
        }
        if let Some(link) = &run.link {
            value["link"] = json!(link);
        }
        value
    }

    fn cell(runs: &[MarkdownRun]) -> Value {
        runs.iter().map(run).collect()
    }

    fn block(block: &MarkdownBlock) -> Value {
        match block {
            MarkdownBlock::Paragraph { runs } => {
                json!({"type": "paragraph", "inlines": cell(runs)})
            }
            MarkdownBlock::Heading { level, runs } => {
                json!({"type": "heading", "level": level, "inlines": cell(runs)})
            }
            MarkdownBlock::Code { language, text } => {
                json!({"type": "code", "language": language, "text": text})
            }
            MarkdownBlock::Quote { blocks } => json!({"type": "quote", "blocks": all(blocks)}),
            MarkdownBlock::ItemList { start, items } => json!({
                "type": "list",
                "ordered": start.is_some(),
                "start": start,
                "items": items.iter().map(|item| all(&item.blocks)).collect::<Vec<_>>(),
            }),
            MarkdownBlock::Table { header, rows } => json!({
                "type": "table",
                "header": header.cells.iter().map(|c| cell(&c.runs)).collect::<Vec<_>>(),
                "rows": rows
                    .iter()
                    .map(|row| row.cells.iter().map(|c| cell(&c.runs)).collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
            }),
            MarkdownBlock::Rule => json!({"type": "rule"}),
        }
    }

    fn all(blocks: &[MarkdownBlock]) -> Value {
        blocks.iter().map(block).collect()
    }

    /// What crosses to the views is the vector's answer, for every case of the shared file: the
    /// projection drops no style, no list number and no code text.
    #[test]
    fn every_shared_vector_reaches_the_views_as_its_blocks() {
        let vectors: Vec<Value> = serde_json::from_str(VECTORS).unwrap();
        assert!(vectors.len() > 50, "the vector file lost its cases");
        for vector in vectors {
            let streaming = vector["streaming"].as_bool().unwrap();
            let parsed = markdown::parse(vector["markdown"].as_str().unwrap(), streaming);
            assert_eq!(
                all(&project(parsed)),
                vector["blocks"],
                "{}",
                vector["name"]
            );
        }
    }

    /// The exported function is that projection of the parse with the bare addresses linked, which
    /// the pane does after the parse (the cases of `web-links.json`) and no vector of this file does.
    #[test]
    fn the_exported_blocks_are_the_parse_with_bare_addresses_linked() {
        let vectors: Vec<Value> = serde_json::from_str(VECTORS).unwrap();
        for vector in vectors {
            let text = vector["markdown"].as_str().unwrap();
            let streaming = vector["streaming"].as_bool().unwrap();
            assert_eq!(
                markdown_blocks(text.to_owned(), streaming),
                project(markdown::link_blocks(markdown::parse(text, streaming))),
                "{}",
                vector["name"]
            );
        }
    }

    #[test]
    fn a_web_address_typed_in_a_sentence_is_a_link_and_one_in_code_is_not() {
        let blocks = markdown_blocks(
            "see https://example.com/a. and `https://x.test`".to_owned(),
            false,
        );
        let [MarkdownBlock::Paragraph { runs }] = blocks.as_slice() else {
            panic!("one paragraph");
        };
        let linked: Vec<_> = runs.iter().filter_map(|run| run.link.as_deref()).collect();
        assert_eq!(linked, ["https://example.com/a"]);
        assert!(runs.iter().any(|run| run.code && run.link.is_none()));
    }

    #[test]
    fn a_code_block_hands_over_the_text_its_copy_button_copies() {
        let blocks = markdown_blocks("```sh\ncd /tmp && ls\n```".to_owned(), false);
        assert_eq!(
            blocks,
            [MarkdownBlock::Code {
                language: Some("sh".to_owned()),
                text: "cd /tmp && ls".to_owned()
            }]
        );
    }

    #[test]
    fn the_copy_button_says_what_the_contract_says() {
        let copy = code_copy();
        assert_eq!(copy.label, "Copy code");
        assert_eq!(copy.copied, "Copied");
        assert_eq!(copy.feedback_millis, 1500);
    }
}
