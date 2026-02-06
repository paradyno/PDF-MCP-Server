//! Source resolution and caching

pub mod cache;
pub mod resolver;

pub use cache::CacheManager;
pub use resolver::{resolve_base64, resolve_cache, resolve_path, resolve_url, ResolvedPdf};
