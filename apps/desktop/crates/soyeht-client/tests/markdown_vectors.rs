//! The Markdown rules every client shares (`contracts/vectors/markdown.json`); the Swift
//! client passes the same file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_client::markdown::{Block, Run, parse};

const VECTORS: &str = include_str!("../../../../../contracts/vectors/markdown.json");

fn run(run: &Run) -> Value {
    let mut value = json!({"text": run.text});
    for (key, on) in [
        ("bold", run.style.bold),
        ("italic", run.style.italic),
        ("strike", run.style.strike),
        ("code", run.style.code),
    ] {
        if on {
            value[key] = json!(true);
        }
    }
    if let Some(link) = &run.style.link {
        value["link"] = json!(link);
    }
    value
}

fn runs(runs: &[Run]) -> Value {
    runs.iter().map(run).collect()
}

fn block(block: &Block) -> Value {
    match block {
        Block::Paragraph(content) => json!({"type": "paragraph", "inlines": runs(content)}),
        Block::Heading {
            level,
            runs: content,
        } => {
            json!({"type": "heading", "level": level, "inlines": runs(content)})
        }
        Block::Code { language, text } => {
            json!({"type": "code", "language": language, "text": text})
        }
        Block::Quote(blocks) => json!({"type": "quote", "blocks": self::blocks(blocks)}),
        Block::List { start, items } => json!({
            "type": "list",
            "ordered": start.is_some(),
            "start": start,
            "items": items.iter().map(|item| blocks(item)).collect::<Vec<_>>(),
        }),
        Block::Table { header, rows } => json!({
            "type": "table",
            "header": header.iter().map(|cell| runs(cell)).collect::<Vec<_>>(),
            "rows": rows
                .iter()
                .map(|row| row.iter().map(|cell| runs(cell)).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
        }),
        Block::Rule => json!({"type": "rule"}),
    }
}

fn blocks(blocks: &[Block]) -> Value {
    blocks.iter().map(block).collect()
}

#[test]
fn every_shared_vector_parses_to_its_blocks() {
    let vectors: Vec<Value> = serde_json::from_str(VECTORS).unwrap();
    assert!(vectors.len() > 50, "the vector file lost its cases");
    for vector in vectors {
        let streaming = vector["streaming"].as_bool().unwrap();
        let parsed = blocks(&parse(vector["markdown"].as_str().unwrap(), streaming));
        assert_eq!(parsed, vector["blocks"], "{}", vector["name"]);
    }
}

/// A reply arrives in pieces: no prefix of a realistic reply may fail to parse, and the
/// finished text parses to the same blocks whether or not it was streamed.
#[test]
fn every_prefix_of_a_reply_parses_while_streaming() {
    let vectors: Vec<Value> = serde_json::from_str(VECTORS).unwrap();
    let reply = vectors
        .iter()
        .find(|vector| vector["name"] == "realistic reply")
        .and_then(|vector| vector["markdown"].as_str())
        .unwrap();
    for end in reply
        .char_indices()
        .map(|(index, _)| index)
        .chain([reply.len()])
    {
        parse(&reply[..end], true);
    }
    assert_eq!(parse(reply, true), parse(reply, false));
}
