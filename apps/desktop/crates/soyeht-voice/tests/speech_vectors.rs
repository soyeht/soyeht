//! Speech text, the sentence chunker, the reply feed and the heard part against
//! `contracts/vectors/speech-text.json`, which the Apple clients pass too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Obj, parse, read_groups};
use serde_json::Value;
use soyeht_protocol::client_policy::{VOICE_SENTENCE_MAX_CHARS, VOICE_SENTENCE_MIN_CHARS};
use soyeht_voice::{ReplyFeed, SentenceChunker, WordTiming, heard_text, speakable};

const VECTORS: &str = include_str!("../../../../../contracts/vectors/speech-text.json");

/// The groups of the file, each an array of cases.
struct Vectors<'a> {
    speech_text: Vec<(String, &'a Value)>,
    chunker: Vec<(String, &'a Value)>,
    stream: Vec<(String, &'a Value)>,
    replies: Vec<(String, &'a Value)>,
    heard: Vec<(String, &'a Value)>,
}

impl<'a> Vectors<'a> {
    fn read(file: &'a Value) -> Self {
        read_groups(file, "speech-text", |groups| Self {
            speech_text: groups.group("speechText"),
            chunker: groups.group("chunker"),
            stream: groups.group("stream"),
            replies: groups.group("replies"),
            heard: groups.group("heard"),
        })
    }
}

fn limit(value: u64) -> usize {
    usize::try_from(value).unwrap()
}

/// The chunker of a case: the policy's limits, or the ones the case overrides.
fn chunker_of(case: &mut Obj<'_>) -> SentenceChunker {
    let mut minimum = limit(VOICE_SENTENCE_MIN_CHARS);
    let mut maximum = limit(VOICE_SENTENCE_MAX_CHARS);
    if let Some(value) = case.optional("config") {
        let mut config = Obj::new(value, format!("{}.config", case.path()));
        if config.has("minimumLength") {
            minimum = limit(u64::try_from(config.integer("minimumLength")).unwrap());
        }
        if config.has("maximumLength") {
            maximum = limit(u64::try_from(config.integer("maximumLength")).unwrap());
        }
        config.done();
    }
    SentenceChunker::new(minimum, maximum)
}

#[test]
fn what_is_spoken_is_what_the_vector_says() {
    let file = parse(VECTORS);
    for (path, value) in &Vectors::read(&file).speech_text {
        let mut case = Obj::new(value, path.as_str());
        let name = case.string("name");
        let markdown = case.string("markdown");
        let streaming = case.optional("streaming").map(|flag| {
            flag.as_bool()
                .unwrap_or_else(|| panic!("{path}.streaming is not a boolean"))
        });
        let spoken = case.string("spoken");
        case.done();
        let actual = speakable(markdown, streaming == Some(true));
        assert_eq!(actual, spoken, "{name}");
    }
}

#[test]
fn sentences_are_cut_as_the_vector_says() {
    let file = parse(VECTORS);
    for (path, value) in &Vectors::read(&file).chunker {
        let mut case = Obj::new(value, path.as_str());
        let name = case.string("name");
        let mut chunker = chunker_of(&mut case);
        for (index, update) in case.array("updates").iter().enumerate() {
            let mut update = Obj::new(update, format!("{path}.updates[{index}]"));
            let spoken = update.string("spoken");
            let is_final = update.boolean("final");
            let emit = update.strings("emit");
            update.done();
            assert_eq!(
                chunker.update(spoken, is_final),
                emit,
                "{name} update {index}"
            );
        }
        case.done();
    }
}

#[test]
fn a_reply_is_spoken_as_it_arrives_as_the_vector_says() {
    let file = parse(VECTORS);
    for (path, value) in &Vectors::read(&file).stream {
        let mut case = Obj::new(value, path.as_str());
        let name = case.string("name");
        let mut chunker = SentenceChunker::default();
        for (index, update) in case.array("updates").iter().enumerate() {
            let mut update = Obj::new(update, format!("{path}.updates[{index}]"));
            let markdown = update.string("markdown");
            let is_final = update.boolean("final");
            let emit = update.strings("emit");
            update.done();
            let spoken = speakable(markdown, !is_final);
            assert_eq!(
                chunker.update(&spoken, is_final),
                emit,
                "{name} update {index}"
            );
        }
        case.done();
    }
}

#[test]
fn replies_are_fed_as_the_vector_says() {
    let file = parse(VECTORS);
    for (path, value) in &Vectors::read(&file).replies {
        let mut case = Obj::new(value, path.as_str());
        let name = case.string("name");
        let mut feed = ReplyFeed::default();
        for (index, step) in case.array("steps").iter().enumerate() {
            let label = format!("{name} step {index}");
            let mut step = Obj::new(step, format!("{path}.steps[{index}]"));
            if step.has("cancel") {
                assert!(step.boolean("cancel"), "{label}: cancel is true or absent");
                feed.cancel();
            } else {
                let mut update = step.object("update");
                let id = update.string("id");
                let markdown = update.string("markdown");
                let is_final = update.boolean("final");
                update.done();
                let result = feed.update(id, markdown, is_final);
                assert_eq!(result.sentences, step.strings("emit"), "{label}: sentences");
                assert_eq!(
                    result.accepted,
                    step.boolean("accepted"),
                    "{label}: accepted"
                );
            }
            assert_eq!(
                feed.has_pending(),
                step.boolean("pending"),
                "{label}: pending"
            );
            step.done();
        }
        case.done();
    }
}

#[test]
fn the_heard_part_is_what_the_vector_says() {
    let file = parse(VECTORS);
    for (path, value) in &Vectors::read(&file).heard {
        let mut case = Obj::new(value, path.as_str());
        let name = case.string("name");
        let played = case.strings("played");
        let current = match case.get("current") {
            Value::Null => None,
            Value::String(text) => Some(text.as_str()),
            other => panic!("{path}.current is not a string or null: {other}"),
        };
        let fraction = case.number("fraction");
        let timings: Option<Vec<WordTiming>> = case.optional("timings").map(|words| {
            words
                .as_array()
                .unwrap_or_else(|| panic!("{path}.timings is not an array"))
                .iter()
                .enumerate()
                .map(|(index, word)| {
                    let mut word = Obj::new(word, format!("{path}.timings[{index}]"));
                    let timing = WordTiming {
                        text: word.string("text").to_owned(),
                        start: word.number("start"),
                    };
                    word.done();
                    timing
                })
                .collect()
        });
        assert_eq!(
            timings.is_some(),
            case.has("position"),
            "{name}: timings and position"
        );
        let position = if timings.is_some() {
            case.number("position")
        } else {
            0.0
        };
        let heard = case.string("heard");
        case.done();
        let actual = heard_text(&played, current, fraction, timings.as_deref(), position);
        assert_eq!(actual, heard, "{name}");
    }
}
