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

/// What the Copy button of a code block puts on the clipboard is the block's text: no fence, no
/// language tag, the closing newline dropped and the whitespace inside kept. The Swift client
/// has the same cases.
#[test]
fn a_code_block_copies_its_text_alone() {
    let command = "cd /Users/me/project && codex";
    let cases = [
        (format!("```sh\n{command}\n```"), command),
        (format!("```\n{command}\n```"), command),
        (format!("~~~bash title=x\n{command}\n~~~"), command),
        (
            "```sh\n  indented  \n\tnext\n```".to_owned(),
            "  indented  \n\tnext",
        ),
        ("before\n\n```\ncode\n```\n\nafter".to_owned(), "code"),
    ];
    for (markdown, copied) in cases {
        let texts: Vec<_> = parse(&markdown, false)
            .into_iter()
            .filter_map(|block| match block {
                Block::Code { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(texts, [copied], "{markdown}");
    }
}
