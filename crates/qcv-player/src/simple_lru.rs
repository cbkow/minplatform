//! `SimpleLRU` — ported from `QCView-Player/src/decode/simple_lru.h` (itself from old QCView's `direct_exr_cache.h`,
//! "tuned over the original year of development"): a byte-budget LRU with O(1) touch. The load-bearing distinction:
//! `get` TOUCHES (the caller intends to keep the entry around — the scrub cache keeps the frames around the playhead hot),
//! `peek` does NOT (playback — already-displayed frames age out naturally). `add` replaces, pushes to MRU, then evicts the
//! oldest while over budget.
use std::{collections::{HashMap, VecDeque}, hash::Hash, sync::Mutex};

struct Inner<K, V> { map: HashMap<K, (V, usize)>, order: VecDeque<K>, max_bytes: usize, bytes: usize }

pub struct SimpleLru<K, V> { inner: Mutex<Inner<K, V>> }

impl<K: Eq + Hash + Clone, V: Clone> SimpleLru<K, V> {
    pub fn new(max_bytes: usize) -> Self { SimpleLru { inner: Mutex::new(Inner { map: HashMap::new(), order: VecDeque::new(), max_bytes, bytes: 0 }) } }
    pub fn set_max_bytes(&self, b: usize) { let mut i = self.inner.lock().unwrap(); i.max_bytes = b; Self::evict(&mut i); }
    pub fn max_bytes(&self) -> usize { self.inner.lock().unwrap().max_bytes }
    pub fn current_bytes(&self) -> usize { self.inner.lock().unwrap().bytes }
    pub fn count(&self) -> usize { self.inner.lock().unwrap().map.len() }
    pub fn contains(&self, k: &K) -> bool { self.inner.lock().unwrap().map.contains_key(k) }
    /// Touches the LRU (moves the entry to MRU).
    pub fn get(&self, k: &K) -> Option<V> {
        let mut i = self.inner.lock().unwrap();
        let v = i.map.get(k)?.0.clone();
        if let Some(p) = i.order.iter().position(|x| x == k) { i.order.remove(p); }
        i.order.push_back(k.clone());
        Some(v)
    }
    /// Does NOT touch the LRU.
    pub fn peek(&self, k: &K) -> Option<V> { self.inner.lock().unwrap().map.get(k).map(|e| e.0.clone()) }
    /// Replaces any entry for `k`, pushes to MRU, evicts the oldest while over budget.
    pub fn add(&self, k: K, v: V, bytes: usize) { self.add_evicting(k, v, bytes); }
    /// [`Self::add`], returning the keys the budget pushed out (QCView's eviction callback runs on them, outside the lock).
    pub fn add_evicting(&self, k: K, v: V, bytes: usize) -> Vec<K> {
        let mut i = self.inner.lock().unwrap();
        if let Some((_, old)) = i.map.remove(&k) { i.bytes -= old; if let Some(p) = i.order.iter().position(|x| *x == k) { i.order.remove(p); } }
        i.map.insert(k.clone(), (v, bytes)); i.bytes += bytes; i.order.push_back(k);
        Self::evict(&mut i)
    }
    /// Every key, unordered (callers that need QCView's `std::map` order sort).
    pub fn keys(&self) -> Vec<K> { self.inner.lock().unwrap().map.keys().cloned().collect() }
    pub fn remove(&self, k: &K) -> Option<V> {
        let mut i = self.inner.lock().unwrap();
        let (v, b) = i.map.remove(k)?; i.bytes -= b;
        if let Some(p) = i.order.iter().position(|x| x == k) { i.order.remove(p); }
        Some(v)
    }
    pub fn clear(&self) { let mut i = self.inner.lock().unwrap(); i.map.clear(); i.order.clear(); i.bytes = 0; }
    fn evict(i: &mut Inner<K, V>) -> Vec<K> {
        let mut out = vec![];
        while i.bytes > i.max_bytes && i.order.len() > 1 {   // never evict the only (just-added) entry
            let Some(k) = i.order.pop_front() else { break };
            if let Some((_, b)) = i.map.remove(&k) { i.bytes -= b; }
            out.push(k);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn budget_and_touch() {
        let l: SimpleLru<i32, &'static str> = SimpleLru::new(30);
        l.add(1, "a", 10); l.add(2, "b", 10); l.add(3, "c", 10);
        assert_eq!(l.count(), 3);
        assert_eq!(l.get(&1), Some("a"));          // 1 becomes MRU
        l.add(4, "d", 10);                          // over budget: evicts the oldest = 2
        assert!(!l.contains(&2) && l.contains(&1) && l.contains(&3) && l.contains(&4));
        assert_eq!(l.peek(&3), Some("c"));         // peek does not touch
        l.add(5, "e", 10);                          // evicts 3 (peek left it oldest)
        assert!(!l.contains(&3) && l.current_bytes() == 30);
        l.add(6, "big", 100); assert!(l.contains(&6) && l.count() == 1);   // the only entry is never evicted
    }
}
