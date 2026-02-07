//! Source resolution for PDF data

use crate::error::{Error, Result};
use crate::source::CacheManager;
use base64::Engine;
use futures_util::StreamExt;
use std::net::IpAddr;
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

/// Check if an IP address is private/reserved (loopback, link-local, private ranges, etc.)
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()                           // 127.0.0.0/8
                || v4.is_private()                     // 10/8, 172.16/12, 192.168/16
                || v4.is_link_local()                  // 169.254/16 (cloud metadata!)
                || v4.is_broadcast()                   // 255.255.255.255
                || v4.is_unspecified()                 // 0.0.0.0
                || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64  // CGNAT 100.64/10
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()                           // ::1
                || v6.is_unspecified()                 // ::
                || {
                    let segments = v6.segments();
                    // fc00::/7 (unique local)
                    (segments[0] & 0xFE00) == 0xFC00
                    // fe80::/10 (link-local)
                    || (segments[0] & 0xFFC0) == 0xFE80
                }
        }
    }
}

/// Check URL for SSRF by resolving DNS and verifying IPs are public
async fn check_ssrf(url_str: &str) -> Result<()> {
    let parsed = url::Url::parse(url_str).map_err(|e| Error::SourceResolution {
        reason: format!("Invalid URL: {}", e),
    })?;

    let host = parsed.host_str().ok_or_else(|| Error::SourceResolution {
        reason: "URL has no host".to_string(),
    })?;

    let port = parsed.port_or_known_default().unwrap_or(443);
    let addr_str = format!("{}:{}", host, port);

    let addrs = tokio::net::lookup_host(&addr_str).await.map_err(|e| {
        Error::SourceResolution {
            reason: format!("DNS resolution failed for {}: {}", host, e),
        }
    })?;

    for addr in addrs {
        if is_private_ip(&addr.ip()) {
            return Err(Error::SsrfBlocked {
                url: url_str.to_string(),
            });
        }
    }

    Ok(())
}

/// Resolve a URL to PDF data with SSRF protection and download size limits
pub async fn resolve_url(
    url: &str,
    allow_private_urls: bool,
    max_download_bytes: u64,
) -> Result<ResolvedPdf> {
    // SSRF check
    if !allow_private_urls {
        check_ssrf(url).await?;
    }

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

    // Check Content-Length header for early rejection
    if let Some(content_length) = response.content_length() {
        if content_length > max_download_bytes {
            return Err(Error::DownloadTooLarge {
                size: content_length,
                max_size: max_download_bytes,
            });
        }
    }

    // Stream the response body with incremental size checking to prevent OOM
    let mut data = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(Error::HttpRequest)?;
        data.extend_from_slice(&chunk);
        if data.len() as u64 > max_download_bytes {
            return Err(Error::DownloadTooLarge {
                size: data.len() as u64,
                max_size: max_download_bytes,
            });
        }
    }

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

    #[test]
    fn test_is_private_ip_loopback() {
        assert!(is_private_ip(&"127.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"127.0.0.2".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_private_ranges() {
        assert!(is_private_ip(&"10.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"172.16.0.1".parse().unwrap()));
        assert!(is_private_ip(&"172.31.255.255".parse().unwrap()));
        assert!(is_private_ip(&"192.168.1.1".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_link_local() {
        // Cloud metadata endpoint range
        assert!(is_private_ip(&"169.254.169.254".parse().unwrap()));
        assert!(is_private_ip(&"169.254.0.1".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_cgnat() {
        assert!(is_private_ip(&"100.64.0.1".parse().unwrap()));
        assert!(is_private_ip(&"100.127.255.255".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_special() {
        assert!(is_private_ip(&"0.0.0.0".parse().unwrap()));
        assert!(is_private_ip(&"255.255.255.255".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_public() {
        assert!(!is_private_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip(&"1.1.1.1".parse().unwrap()));
        assert!(!is_private_ip(&"203.0.113.1".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_ipv6() {
        assert!(is_private_ip(&"::1".parse().unwrap()));
        assert!(is_private_ip(&"::".parse().unwrap()));
        assert!(is_private_ip(&"fc00::1".parse().unwrap()));
        assert!(is_private_ip(&"fd00::1".parse().unwrap()));
        assert!(is_private_ip(&"fe80::1".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_ipv6_public() {
        assert!(!is_private_ip(&"2001:db8::1".parse().unwrap()));
        assert!(!is_private_ip(
            &"2607:f8b0:4004:800::200e".parse().unwrap()
        ));
    }
}
