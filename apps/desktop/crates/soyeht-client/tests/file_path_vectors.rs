//! The file references every client links in a reply or in terminal output
//! (`contracts/vectors/file-paths.json`); the Swift client passes the same file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;
use soyeht_client::file_links::{Reference, find};

const VECTORS: &str = include_str!("../../../../../contracts/vectors/file-paths.json");

#[test]
fn every_shared_vector_finds_the_same_references() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let cases = vectors["cases"].as_array().unwrap();
    assert!(cases.len() > 10, "the vector file lost its cases");
    for case in cases {
        let text = case["text"].as_str().unwrap();
        let found: Vec<(String, Reference)> = find(text)
            .into_iter()
            .map(|found| (text[found.range].to_owned(), found.reference))
            .collect();
        let expected: Vec<(String, Reference)> = case["references"]
            .as_array()
            .unwrap()
            .iter()
            .map(|reference| {
                (
                    reference["label"].as_str().unwrap().to_owned(),
                    Reference {
                        path: reference["path"].as_str().unwrap().to_owned(),
                        line: u32::try_from(reference["line"].as_u64().unwrap()).unwrap(),
                        column: reference["column"]
                            .as_u64()
                            .map(|column| u32::try_from(column).unwrap()),
                    },
                )
            })
            .collect();
        assert_eq!(found, expected, "{}", case["name"]);
    }
}
