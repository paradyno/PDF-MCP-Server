//! PDF caching layer

use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;

/// Cache manager for PDF data
pub struct CacheManager {
    cache: Mutex<LruCache<String, Vec<u8>>>,
}

impl CacheManager {
    /// Create a new cache manager with the specified capacity
    pub fn new(capacity: usize) -> Self {
        let capacity = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(1).unwrap());
        Self {
            cache: Mutex::new(LruCache::new(capacity)),
        }
    }

    /// Store PDF data in the cache
    pub fn put(&self, key: String, data: Vec<u8>) {
        self.cache.lock().put(key, data);
    }

    /// Get PDF data from the cache
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.cache.lock().get(key).cloned()
    }

    /// Check if a key exists in the cache
    pub fn contains(&self, key: &str) -> bool {
        self.cache.lock().contains(key)
    }

    /// Remove an entry from the cache
    pub fn remove(&self, key: &str) -> Option<Vec<u8>> {
        self.cache.lock().pop(key)
    }

    /// Clear all entries from the cache
    pub fn clear(&self) {
        self.cache.lock().clear();
    }

    /// Get the number of entries in the cache
    pub fn len(&self) -> usize {
        self.cache.lock().len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.lock().is_empty()
    }

    /// Generate a new cache key
    pub fn generate_key() -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_basic_operations() {
        let cache = CacheManager::new(10);

        assert!(cache.is_empty());

        cache.put("key1".to_string(), vec![1, 2, 3]);
        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 1);

        let data = cache.get("key1").unwrap();
        assert_eq!(data, vec![1, 2, 3]);

        assert!(cache.contains("key1"));
        assert!(!cache.contains("key2"));
    }

    #[test]
    fn test_cache_eviction() {
        let cache = CacheManager::new(2);

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
        let cache = CacheManager::new(10);

        cache.put("key1".to_string(), vec![1, 2, 3]);
        let removed = cache.remove("key1");

        assert_eq!(removed, Some(vec![1, 2, 3]));
        assert!(!cache.contains("key1"));
    }

    #[test]
    fn test_cache_clear() {
        let cache = CacheManager::new(10);

        cache.put("key1".to_string(), vec![1]);
        cache.put("key2".to_string(), vec![2]);

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_generate_key() {
        let key1 = CacheManager::generate_key();
        let key2 = CacheManager::generate_key();

        assert_ne!(key1, key2);
        assert_eq!(key1.len(), 36); // UUID format
    }
}
