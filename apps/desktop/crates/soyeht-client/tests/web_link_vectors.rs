//! The web addresses every client links in a reply (`contracts/vectors/web-links.json`); the
//! Swift client passes the same file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;
use soyeht_client::markdown::bare_addresses;

const VECTORS: &str = include_str!("../../../../../contracts/vectors/web-links.json");

#[test]
fn every_shared_vector_links_the_same_addresses() {
    let vectors: Vec<Value> = serde_json::from_str(VECTORS).unwrap();
    assert!(vectors.len() > 15, "the vector file lost its cases");
    for vector in vectors {
        let text = vector["text"].as_str().unwrap();
        let found: Vec<&str> = bare_addresses(text)
            .into_iter()
            .map(|range| &text[range])
            .collect();
        let expected: Vec<&str> = vector["links"]
            .as_array()
            .unwrap()
            .iter()
            .map(|link| link.as_str().unwrap())
            .collect();
        assert_eq!(found, expected, "{}", vector["name"]);
    }
}
