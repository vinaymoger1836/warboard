//! Collections allowed in deterministic code.
//!
//! Hash-map iteration order varies between runs and platforms, so clippy bans
//! `HashMap`/`HashSet` (see `clippy.toml`). Use `BTreeMap`/`BTreeSet` or a
//! sorted `Vec` when you iterate; use [`LookupMap`] when you only need O(1)
//! lookups. `LookupMap` has no iteration API, so it cannot leak order.

#![allow(clippy::disallowed_types)]

use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, DefaultHasher, Hash};

pub use std::collections::{BTreeMap, BTreeSet};

/// A hash map you can query but never iterate.
#[derive(Clone, Debug)]
pub struct LookupMap<K, V> {
    inner: HashMap<K, V, BuildHasherDefault<DefaultHasher>>,
}

impl<K, V> Default for LookupMap<K, V> {
    fn default() -> Self {
        Self {
            inner: HashMap::default(),
        }
    }
}

impl<K: Eq + Hash, V> LookupMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: HashMap::with_capacity_and_hasher(capacity, BuildHasherDefault::default()),
        }
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.inner.insert(key, value)
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.get(key)
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.get_mut(key)
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.remove(key)
    }

    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::LookupMap;

    #[test]
    fn lookup_only() {
        let mut m = LookupMap::new();
        assert!(m.insert(3_u32, "c").is_none());
        m.insert(1, "a");
        assert_eq!(m.get(&3), Some(&"c"));
        assert_eq!(m.remove(&1), Some("a"));
        assert!(!m.contains_key(&1));
        assert_eq!(m.len(), 1);
    }
}
