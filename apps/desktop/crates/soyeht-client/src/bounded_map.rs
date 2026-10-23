//! A map that keeps a bounded number of entries and, when it is full, drops the oldest.
//!
//! Every cache of the client is one of these (the parsed replies, the answers about files), so
//! that they all hold the same number of entries and all make room the same way: one entry at a
//! time, the one that was written longest ago. Reading an entry does not renew it; writing it
//! again does.

use std::{
    borrow::Borrow,
    collections::{BTreeMap, HashMap},
    hash::Hash,
};

/// The most entries a cache of the client keeps: more than a transcript shows at once.
pub const CACHE_ENTRIES: usize = 1024;

pub struct BoundedMap<K, V> {
    entries: HashMap<K, (u64, V)>,
    /// The keys by the order they were written, oldest first.
    order: BTreeMap<u64, K>,
    written: u64,
}

impl<K, V> Default for BoundedMap<K, V> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            order: BTreeMap::new(),
            written: 0,
        }
    }
}

impl<K: Clone + Eq + Hash, V> BoundedMap<K, V> {
    pub fn get<Q: Hash + Eq + ?Sized>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.entries.get(key).map(|(_, value)| value)
    }

    /// Writes `value` under `key` as the newest entry. A full map drops its oldest entry first,
    /// unless `key` is already there.
    pub fn insert(&mut self, key: K, value: V) {
        if let Some((written, _)) = self.entries.remove(&key) {
            self.order.remove(&written);
        } else if self.entries.len() >= CACHE_ENTRIES
            && let Some((_, oldest)) = self.order.pop_first()
        {
            self.entries.remove(&oldest);
        }
        self.written += 1;
        self.order.insert(self.written, key.clone());
        self.entries.insert(key, (self.written, value));
    }

    pub fn remove<Q: Hash + Eq + ?Sized>(&mut self, key: &Q)
    where
        K: Borrow<Q>,
    {
        if let Some((written, _)) = self.entries.remove(key) {
            self.order.remove(&written);
        }
    }

    #[cfg(test)]
    pub fn kept(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_map_drops_its_oldest_entry_and_keeps_the_rest() {
        let mut map = BoundedMap::default();
        for index in 0..CACHE_ENTRIES {
            map.insert(index, index);
        }
        assert_eq!(map.kept(), CACHE_ENTRIES);
        map.insert(CACHE_ENTRIES, CACHE_ENTRIES);
        assert_eq!(map.kept(), CACHE_ENTRIES);
        assert_eq!(map.get(&0), None);
        assert_eq!(map.get(&1), Some(&1));
        assert_eq!(map.get(&CACHE_ENTRIES), Some(&CACHE_ENTRIES));
    }

    #[test]
    fn writing_an_entry_again_renews_it_and_makes_no_room_to_make() {
        let mut map = BoundedMap::default();
        for index in 0..CACHE_ENTRIES {
            map.insert(index, index);
        }
        map.insert(0, 100);
        assert_eq!(map.kept(), CACHE_ENTRIES);
        assert_eq!(map.get(&0), Some(&100));
        // 0 is now the newest, so the next to go is 1.
        map.insert(CACHE_ENTRIES, 0);
        assert_eq!(map.get(&1), None);
        assert_eq!(map.get(&0), Some(&100));
    }

    #[test]
    fn a_removed_entry_is_gone_and_leaves_the_others() {
        let mut map = BoundedMap::default();
        map.insert("a", 1);
        map.insert("b", 2);
        map.remove("a");
        map.remove("never there");
        assert_eq!(map.get("a"), None);
        assert_eq!(map.get("b"), Some(&2));
        assert_eq!(map.kept(), 1);
    }
}
