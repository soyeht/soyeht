//! What a read finds on each screen, kept once and within the contract's limits: the visible
//! text, grouped by block, and the visible `http` and `https` links. Screens overlap and a fixed
//! header repeats on each of them, so a block already seen at the same page position, or a pinned
//! block with the same words, is kept once. The rules are those of the Mac client
//! (`WebReadAccumulator`), and both pass `contracts/vectors/web-read.json`.

use serde::Deserialize;
use soyeht_protocol::{WebReadLink, limits};
use std::collections::HashSet;

/// A run of visible text the page script found on one screen.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Block {
    pub y: i64,
    pub text: String,
    pub pinned: bool,
}

#[derive(Debug, Default)]
pub struct Accumulator {
    lines: Vec<String>,
    links: Vec<WebReadLink>,
    truncated: bool,
    text_bytes: usize,
    seen_blocks: HashSet<String>,
    seen_links: HashSet<String>,
}

impl Accumulator {
    pub fn add(&mut self, blocks: &[Block], found: &[WebReadLink]) {
        for block in blocks.iter().filter(|block| !block.text.is_empty()) {
            let key = if block.pinned {
                format!("pinned|{}", block.text)
            } else {
                format!("{}|{}", block.y, block.text)
            };
            if !self.seen_blocks.insert(key) {
                continue;
            }
            let cost = block.text.len() + usize::from(!self.lines.is_empty());
            if self.text_bytes + cost > limits::WEB_READ_TEXT_BYTES {
                self.truncated = true;
                continue;
            }
            self.lines.push(block.text.clone());
            self.text_bytes += cost;
        }
        for link in found {
            let Some(bounded) = self.bounded(link) else {
                continue;
            };
            if !self
                .seen_links
                .insert(format!("{}|{}", bounded.href, bounded.text))
            {
                continue;
            }
            if self.links.len() >= limits::WEB_READ_LINKS {
                self.truncated = true;
                continue;
            }
            self.links.push(bounded);
        }
    }

    fn bounded(&mut self, link: &WebReadLink) -> Option<WebReadLink> {
        if link.href.is_empty() {
            return None;
        }
        if link.href.len() > limits::WEB_FIELD_BYTES {
            self.truncated = true;
            return None;
        }
        let mut text = link.text.clone();
        while text.len() > limits::WEB_TOKEN_BYTES {
            text.pop();
        }
        Some(WebReadLink {
            text,
            href: link.href.clone(),
        })
    }

    #[cfg(test)]
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    #[cfg(test)]
    pub fn links(&self) -> &[WebReadLink] {
        &self.links
    }

    #[cfg(test)]
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Says that something was cut that this keeps no record of (the picture, the byte budget).
    pub fn cut(&mut self) {
        self.truncated = true;
    }

    /// Takes what is kept: the text, the links and whether anything was cut.
    pub fn into_parts(self) -> (String, Vec<WebReadLink>, bool) {
        (self.lines.join("\n"), self.links, self.truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::vector;
    use serde_json::Value;

    fn block(y: i64, text: &str, pinned: bool) -> Block {
        Block {
            y,
            text: text.into(),
            pinned,
        }
    }

    fn link(text: &str, href: &str) -> WebReadLink {
        WebReadLink {
            text: text.into(),
            href: href.into(),
        }
    }

    /// The cases the Mac client passes too.
    #[test]
    fn the_vector_cases_of_the_accumulator() {
        let cases = vector("web-read");
        for case in cases["accumulate"].as_array().unwrap() {
            let mut accumulator = Accumulator::default();
            for step in case["steps"].as_array().unwrap() {
                let blocks: Vec<Block> = serde_json::from_value(step["blocks"].clone()).unwrap();
                let links: Vec<WebReadLink> =
                    serde_json::from_value(step["links"].clone()).unwrap();
                accumulator.add(&blocks, &links);
            }
            let expect = &case["expect"];
            let name = &case["name"];
            assert_eq!(Value::from(accumulator.text()), expect["text"], "{name}");
            assert_eq!(
                accumulator.links().len() as u64,
                expect["links"].as_u64().unwrap(),
                "{name}"
            );
            assert_eq!(
                Value::from(accumulator.truncated()),
                expect["truncated"],
                "{name}"
            );
        }
    }

    #[test]
    fn text_and_links_stay_inside_their_limits() {
        let mut accumulator = Accumulator::default();
        let line = "w".repeat(1_000);
        let blocks: Vec<Block> = (0..100)
            .map(|index| block(index, &format!("{index} {line}"), false))
            .collect();
        let links: Vec<WebReadLink> = (0..150)
            .map(|index| {
                link(
                    &format!("link {index}"),
                    &format!("https://example.test/{index}"),
                )
            })
            .collect();
        accumulator.add(&blocks, &links);
        assert!(accumulator.text().len() <= limits::WEB_READ_TEXT_BYTES);
        assert_eq!(accumulator.links().len(), limits::WEB_READ_LINKS);
        assert!(accumulator.truncated());
    }

    #[test]
    fn links_are_bounded_one_by_one() {
        let mut accumulator = Accumulator::default();
        let too_long = format!(
            "https://example.test/{}",
            "a".repeat(limits::WEB_FIELD_BYTES)
        );
        accumulator.add(
            &[],
            &[
                link(&"t".repeat(1_000), "https://example.test/ok"),
                link("dropped", &too_long),
                link("same", "https://example.test/ok"),
                link("same", "https://example.test/ok"),
            ],
        );
        assert_eq!(accumulator.links().len(), 2);
        assert_eq!(accumulator.links()[0].text.len(), limits::WEB_TOKEN_BYTES);
        assert!(accumulator.truncated());
    }
}
