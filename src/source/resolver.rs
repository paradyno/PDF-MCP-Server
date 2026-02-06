//! Source resolution for PDF data

use crate::error::{Error, Result};
use crate::source::CacheManager;
use base64::Engine;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Resolved PDF data
pub struct ResolvedPdf {
    pub data: Vec<u8>,
    pub source_name: String,
}

/// Resolve a file path to PDF data
pub fn resolve_path<P: AsRef<Path>>(path: P) -> Result<ResolvedPdf> {
    let path = path.as_ref();

    if !path.exists() {
        return Err(Error::PdfNotFound {
            path: path.display().to_string(),
        });
    }

    let data = std::fs::read(path).map_err(Error::Io)?;

    // Validate PDF header
    if data.len() < 4 || &data[0..4] != b"%PDF" {
        return Err(Error::InvalidPdf {
            reason: "Not a valid PDF file".to_string(),
        });
    }

    Ok(ResolvedPdf {
        data,
        source_name: path.display().to_string(),
    })
}

/// Resolve base64 encoded data to PDF data
pub fn resolve_base64(base64_data: &str) -> Result<ResolvedPdf> {
    let engine = base64::engine::general_purpose::STANDARD;
    let data = engine.decode(base64_data)?;

    // Validate PDF header
    if data.len() < 4 || &data[0..4] != b"%PDF" {
        return Err(Error::InvalidPdf {
            reason: "Decoded data is not a valid PDF file".to_string(),
        });
    }

    Ok(ResolvedPdf {
        data,
        source_name: "<base64>".to_string(),
    })
}

/// Resolve a URL to PDF data
pub async fn resolve_url(url: &str) -> Result<ResolvedPdf> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(Error::HttpRequest)?;

    let response = client.get(url).send().await?;

    if !response.status().is_success() {
        return Err(Error::SourceResolution {
            reason: format!("HTTP request failed with status: {}", response.status()),
        });
    }

    let data = response.bytes().await?.to_vec();

    // Validate PDF header
    if data.len() < 4 || &data[0..4] != b"%PDF" {
        return Err(Error::InvalidPdf {
            reason: "Downloaded data is not a valid PDF file".to_string(),
        });
    }

    Ok(ResolvedPdf {
        data,
        source_name: url.to_string(),
    })
}

/// Resolve a cache key to PDF data
pub async fn resolve_cache(
    cache_key: &str,
    cache: &Arc<RwLock<CacheManager>>,
) -> Result<ResolvedPdf> {
    let cache_guard = cache.read().await;
    let data = cache_guard
        .get(cache_key)
        .ok_or_else(|| Error::CacheKeyNotFound {
            key: cache_key.to_string(),
        })?;

    Ok(ResolvedPdf {
        data,
        source_name: format!("<cache:{}>", cache_key),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_base64_invalid() {
        // Valid base64 but not PDF
        let result = resolve_base64("SGVsbG8gV29ybGQ="); // "Hello World"
        assert!(matches!(result, Err(Error::InvalidPdf { .. })));
    }

    #[test]
    fn test_resolve_base64_invalid_base64() {
        let result = resolve_base64("not valid base64!!!");
        assert!(matches!(result, Err(Error::Base64Decode(_))));
    }

    #[test]
    fn test_resolve_path_not_found() {
        let result = resolve_path("/nonexistent/path/file.pdf");
        assert!(matches!(result, Err(Error::PdfNotFound { .. })));
    }
}
