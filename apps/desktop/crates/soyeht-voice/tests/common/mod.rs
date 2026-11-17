//! A strict reader for the vector files: a key that a case needs and does not have, a key that it
//! has and nothing reads, an unknown kind and a group that nothing runs fail the test. There is no
//! default for a missing key.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]

use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub const POLICY: &str = "x-clientPolicy.voice";

pub fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("a vector file is JSON")
}

/// One JSON object, read key by key. `done` fails when a key was not read.
pub struct Obj<'a> {
    path: String,
    map: &'a Map<String, Value>,
    read: BTreeSet<&'a str>,
}

impl<'a> Obj<'a> {
    pub fn new(value: &'a Value, path: impl Into<String>) -> Self {
        let path = path.into();
        let map = value
            .as_object()
            .unwrap_or_else(|| panic!("{path} is not an object"));
        Self {
            path,
            map,
            read: BTreeSet::new(),
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn has(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// The key's value; `null` is a value like any other.
    pub fn get(&mut self, key: &'static str) -> &'a Value {
        let (name, value) = self
            .map
            .get_key_value(key)
            .unwrap_or_else(|| panic!("{} needs the key {key}", self.path));
        self.read.insert(name.as_str());
        value
    }

    pub fn optional(&mut self, key: &'static str) -> Option<&'a Value> {
        self.has(key).then(|| self.get(key))
    }

    pub fn string(&mut self, key: &'static str) -> &'a str {
        let value = self.get(key);
        value
            .as_str()
            .unwrap_or_else(|| panic!("{}.{key} is not a string", self.path))
    }

    pub fn integer(&mut self, key: &'static str) -> i64 {
        let value = self.get(key);
        value
            .as_i64()
            .unwrap_or_else(|| panic!("{}.{key} is not an integer", self.path))
    }

    pub fn number(&mut self, key: &'static str) -> f64 {
        let value = self.get(key);
        value
            .as_f64()
            .unwrap_or_else(|| panic!("{}.{key} is not a number", self.path))
    }

    pub fn boolean(&mut self, key: &'static str) -> bool {
        let value = self.get(key);
        value
            .as_bool()
            .unwrap_or_else(|| panic!("{}.{key} is not a boolean", self.path))
    }

    pub fn array(&mut self, key: &'static str) -> &'a [Value] {
        let value = self.get(key);
        value
            .as_array()
            .unwrap_or_else(|| panic!("{}.{key} is not an array", self.path))
    }

    pub fn strings(&mut self, key: &'static str) -> Vec<String> {
        let path = format!("{}.{key}", self.path);
        self.array(key)
            .iter()
            .map(|item| {
                item.as_str()
                    .unwrap_or_else(|| panic!("{path} holds a value that is not a string"))
                    .to_owned()
            })
            .collect()
    }

    pub fn object(&mut self, key: &'static str) -> Obj<'a> {
        let path = format!("{}.{key}", self.path);
        Obj::new(self.get(key), path)
    }

    /// Every key with its value, all of them read.
    pub fn entries(&mut self) -> Vec<(&'a str, &'a Value)> {
        let map = self.map;
        let mut entries = Vec::new();
        for (key, value) in map {
            self.read.insert(key.as_str());
            entries.push((key.as_str(), value));
        }
        entries
    }

    /// Fails when the object has a key that was not read: a misspelled or an unknown one.
    pub fn done(self) {
        let unread: Vec<&String> = self
            .map
            .keys()
            .filter(|key| !self.read.contains(key.as_str()))
            .collect();
        assert!(
            unread.is_empty(),
            "{} has keys nothing reads: {unread:?}",
            self.path
        );
    }
}

/// The top level of a vector file: its policy tag and the groups it holds, each of which the test
/// must run.
pub struct Groups<'a> {
    root: Obj<'a>,
}

impl<'a> Groups<'a> {
    pub fn new(file: &'a Value, name: &str) -> Self {
        let mut root = Obj::new(file, name);
        assert_eq!(root.string("policy"), POLICY, "{name} policy");
        Self { root }
    }

    /// The elements of a group, which must not be empty.
    pub fn group(&mut self, key: &'static str) -> Vec<(String, &'a Value)> {
        let path = format!("{}.{key}", self.root.path());
        let items = self.root.array(key);
        assert!(!items.is_empty(), "{path} has no case");
        items
            .iter()
            .enumerate()
            .map(|(index, item)| (format!("{path}[{index}]"), item))
            .collect()
    }

    /// A group that is an object of named lists.
    pub fn object(&mut self, key: &'static str) -> Obj<'a> {
        self.root.object(key)
    }

    /// Fails when the file has a group the test did not run.
    pub fn done(self) {
        self.root.done();
    }
}
