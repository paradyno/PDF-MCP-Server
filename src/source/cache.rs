//! PDF caching layer

use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;

struct CacheInner {
    lru: LruCache<String, Vec<u8>>,
    total_bytes: usize,
}

/// Cache manager for PDF data with entry count and byte budget limits
pub struct CacheManager {
    inner: Mutex<CacheInner>,
    max_bytes: usize,
}

impl CacheManager {
    /// Create a new cache manager with the specified entry capacity and byte budget
    pub fn new(capacity: usize, max_bytes: usize) -> Self {
        let capacity = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(1).unwrap());
        Self {
            inner: Mutex::new(CacheInner {
                lru: LruCache::new(capacity),
                total_bytes: 0,
            }),
            max_bytes,
        }
    }

    /// Store PDF data in the cache.
    /// Rejects entries larger than max_bytes entirely.
    /// Evicts LRU entries until byte budget is satisfied.
    pub fn put(&self, key: String, data: Vec<u8>) {
        let new_size = data.len();

        // Reject single entries that exceed the entire budget
        if new_size > self.max_bytes {
            return;
        }

        let mut inner = self.inner.lock();

        // If updating an existing key, subtract old size first
        if let Some(old) = inner.lru.pop(&key) {
            inner.total_bytes = inner.total_bytes.saturating_sub(old.len());
        }

        // Evict LRU entries until we have room
        while inner.total_bytes + new_size > self.max_bytes {
            if let Some((_evicted_key, evicted_val)) = inner.lru.pop_lru() {
                inner.total_bytes = inner.total_bytes.saturating_sub(evicted_val.len());
            } else {
                break;
            }
        }

        inner.total_bytes += new_size;
        inner.lru.put(key, data);
    }

    /// Get PDF data from the cache
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.inner.lock().lru.get(key).cloned()
    }

    /// Check if a key exists in the cache
    pub fn contains(&self, key: &str) -> bool {
        self.inner.lock().lru.contains(key)
    }

    /// Remove an entry from the cache
    pub fn remove(&self, key: &str) -> Option<Vec<u8>> {
        let mut inner = self.inner.lock();
        if let Some(val) = inner.lru.pop(key) {
            inner.total_bytes = inner.total_bytes.saturating_sub(val.len());
            Some(val)
        } else {
            None
        }
    }

    /// Clear all entries from the cache
    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.lru.clear();
        inner.total_bytes = 0;
    }

    /// Get the number of entries in the cache
    pub fn len(&self) -> usize {
        self.inner.lock().lru.len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.inner.lock().lru.is_empty()
    }

    /// Get total bytes currently stored in cache
    pub fn total_bytes(&self) -> usize {
        self.inner.lock().total_bytes
    }

    /// Generate a new cache key (static, no collision check)
    pub fn generate_key() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    /// Generate a new cache key that is guaranteed not to collide with existing keys.
    pub fn generate_unique_key(&self) -> String {
        let inner = self.inner.lock();
        loop {
            let key = uuid::Uuid::new_v4().to_string();
            if !inner.lru.contains(&key) {
                return key;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_basic_operations() {
        let cache = CacheManager::new(10, 1024 * 1024);

        assert!(cache.is_empty());

        cache.put("key1".to_string(), vec![1, 2, 3]);
        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.total_bytes(), 3);

        let data = cache.get("key1").unwrap();
        assert_eq!(data, vec![1, 2, 3]);

        assert!(cache.contains("key1"));
        assert!(!cache.contains("key2"));
    }

    #[test]
    fn test_cache_eviction() {
        let cache = CacheManager::new(2, 1024 * 1024);

        cache.put("key1".to_string(), vec![1]);
        cache.put("key2".to_string(), vec![2]);
        cache.put("key3".to_string(), vec![3]);

        // key1 should be evicted (LRU)
        assert!(!cache.contains("key1"));
        assert!(cache.contains("key2"));
        assert!(cache.contains("key3"));
    }

    #[test]
    fn test_cache_remove() {
        let cache = CacheManager::new(10, 1024 * 1024);

        cache.put("key1".to_string(), vec![1, 2, 3]);
        assert_eq!(cache.total_bytes(), 3);

        let removed = cache.remove("key1");
        assert_eq!(removed, Some(vec![1, 2, 3]));
        assert!(!cache.contains("key1"));
        assert_eq!(cache.total_bytes(), 0);
    }

    #[test]
    fn test_cache_clear() {
        let cache = CacheManager::new(10, 1024 * 1024);

        cache.put("key1".to_string(), vec![1]);
        cache.put("key2".to_string(), vec![2]);
        assert_eq!(cache.total_bytes(), 2);

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.total_bytes(), 0);
    }

    #[test]
    fn test_generate_key() {
        let key1 = CacheManager::generate_key();
        let key2 = CacheManager::generate_key();

        assert_ne!(key1, key2);
        assert_eq!(key1.len(), 36); // UUID format
    }

    #[test]
    fn test_cache_byte_budget_eviction() {
        // 100 byte budget, 10 entry capacity
        let cache = CacheManager::new(10, 100);

        // Put 5 entries of 30 bytes each = 150, exceeds budget
        cache.put("key1".to_string(), vec![0u8; 30]);
        cache.put("key2".to_string(), vec![0u8; 30]);
        cache.put("key3".to_string(), vec![0u8; 30]);
        assert_eq!(cache.total_bytes(), 90);

        // Adding 30 more would exceed 100, so key1 should be evicted
        cache.put("key4".to_string(), vec![0u8; 30]);
        assert!(!cache.contains("key1"));
        assert!(cache.contains("key2"));
        assert!(cache.contains("key3"));
        assert!(cache.contains("key4"));
        assert_eq!(cache.total_bytes(), 90);
    }

    #[test]
    fn test_cache_oversized_entry_rejected() {
        let cache = CacheManager::new(10, 50);

        // Entry larger than entire budget should be rejected
        cache.put("huge".to_string(), vec![0u8; 100]);
        assert!(!cache.contains("huge"));
        assert_eq!(cache.total_bytes(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_byte_tracking_update() {
        let cache = CacheManager::new(10, 1024);

        cache.put("key1".to_string(), vec![0u8; 50]);
        assert_eq!(cache.total_bytes(), 50);

        // Updating same key should adjust bytes
        cache.put("key1".to_string(), vec![0u8; 30]);
        assert_eq!(cache.total_bytes(), 30);
        assert_eq!(cache.len(), 1);
    }
}
