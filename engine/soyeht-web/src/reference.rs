use std::collections::BTreeSet;

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationReferences {
    generation: u64,
    next: u64,
    issued: BTreeSet<String>,
}

impl NavigationReferences {
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            next: 1,
            issued: BTreeSet::new(),
        }
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn issue(&mut self) -> Result<String> {
        let reference = self.next_reference();
        self.next = self.next.checked_add(1).ok_or(Error::ReferenceExhausted)?;
        self.issued.insert(reference.clone());
        Ok(reference)
    }

    pub(crate) fn next_reference(&self) -> String {
        format!("e{}", self.next)
    }

    pub fn contains(&self, generation: u64, reference: &str) -> Result<bool> {
        if generation != self.generation {
            return Err(Error::StaleReference);
        }
        Ok(self.issued.contains(reference))
    }
}
