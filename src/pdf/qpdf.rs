//! qpdf FFI wrapper for PDF manipulation
//!
//! This module provides PDF page operations like splitting, merging,
//! encryption/decryption, and compression using the qpdf crate (vendored FFI).

use crate::error::{Error, Result};
use qpdf::{EncryptionParams, EncryptionParamsR6, ObjectStreamMode, PrintPermission, QPdf};

/// Wrapper for qpdf operations via FFI
pub struct QpdfWrapper;

/// Parse a qpdf-compatible page range string into 0-indexed page indices.
///
/// Supports:
/// - `N` (single page, 1-indexed)
/// - `N-M` (range)
/// - `z` (last page), `rN` (N-th from last)
/// - `z-1` (reverse all pages)
/// - `N-M:odd`, `N-M:even` (odd/even filter)
/// - Comma-separated combinations
fn parse_qpdf_page_range(range: &str, num_pages: u32) -> Result<Vec<u32>> {
    if num_pages == 0 {
        return Err(Error::QpdfError {
            reason: "PDF has no pages".to_string(),
        });
    }

    let mut all_indices: Vec<u32> = Vec::new();

    for part in range.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Check for :odd or :even modifier
        let (range_part, modifier) = if let Some(r) = part.strip_suffix(":odd") {
            (r, Some("odd"))
        } else if let Some(r) = part.strip_suffix(":even") {
            (r, Some("even"))
        } else {
            (part, None)
        };

        // Parse the range part into a list of 1-indexed page numbers
        let pages = if range_part.contains('-') {
            // Range: N-M
            let parts: Vec<&str> = range_part.splitn(2, '-').collect();
            let start = resolve_page_ref(parts[0], num_pages)?;
            let end = resolve_page_ref(parts[1], num_pages)?;

            if start <= end {
                (start..=end).collect::<Vec<u32>>()
            } else {
                // Reverse range
                (end..=start).rev().collect::<Vec<u32>>()
            }
        } else {
            // Single page
            let page = resolve_page_ref(range_part, num_pages)?;
            vec![page]
        };

        // Apply odd/even modifier
        let filtered = match modifier {
            Some("odd") => pages.into_iter().filter(|p| p % 2 == 1).collect(),
            Some("even") => pages.into_iter().filter(|p| p % 2 == 0).collect(),
            _ => pages,
        };

        all_indices.extend(filtered);
    }

    if all_indices.is_empty() {
        return Err(Error::InvalidPageRange {
            range: range.to_string(),
        });
    }

    // Convert to 0-indexed
    Ok(all_indices.iter().map(|p| p - 1).collect())
}

/// Resolve a page reference (1-indexed) to a page number.
/// Handles: numeric "N", "z" (last), "rN" (N-th from last)
fn resolve_page_ref(s: &str, num_pages: u32) -> Result<u32> {
    let s = s.trim();
    if s == "z" {
        return Ok(num_pages);
    }
    if let Some(r_num) = s.strip_prefix('r') {
        let n: u32 = r_num.parse().map_err(|_| Error::InvalidPageRange {
            range: s.to_string(),
        })?;
        if n == 0 || n > num_pages {
            return Err(Error::InvalidPageRange {
                range: s.to_string(),
            });
        }
        return Ok(num_pages - n + 1);
    }
    let page: u32 = s.parse().map_err(|_| Error::InvalidPageRange {
        range: s.to_string(),
    })?;
    if page == 0 || page > num_pages {
        return Err(Error::InvalidPageRange {
            range: format!("page {} out of range (1-{})", page, num_pages),
        });
    }
    Ok(page)
}

/// Helper: open a QPdf from memory, optionally with password
fn open_qpdf(data: &[u8], password: Option<&str>) -> Result<QPdf> {
    match password {
        Some(pwd) => QPdf::read_from_memory_encrypted(data, pwd).map_err(map_qpdf_error),
        None => QPdf::read_from_memory(data).map_err(map_qpdf_error),
    }
}

/// Map qpdf crate errors to our error types
fn map_qpdf_error(e: qpdf::QPdfError) -> Error {
    match e.error_code() {
        qpdf::QPdfErrorCode::InvalidPassword => Error::IncorrectPassword,
        _ => Error::QpdfError {
            reason: e.to_string(),
        },
    }
}

impl QpdfWrapper {
    /// Extract specific pages from a PDF
    ///
    /// # Arguments
    /// * `input_data` - Raw PDF bytes
    /// * `pages` - Page range specification in qpdf format (e.g., "1-5,10", "z-1", ":odd")
    /// * `password` - Optional password for encrypted PDFs
    ///
    /// # Returns
    /// The extracted pages as a new PDF in bytes
    pub fn split_pages(input_data: &[u8], pages: &str, password: Option<&str>) -> Result<Vec<u8>> {
        let source = open_qpdf(input_data, password)?;
        let num_pages = source.get_num_pages().map_err(map_qpdf_error)?;

        let indices = parse_qpdf_page_range(pages, num_pages)?;

        let dest = QPdf::empty();

        for &idx in &indices {
            let page = source.get_page(idx).ok_or_else(|| Error::PageOutOfBounds {
                page: idx + 1,
                total: num_pages,
            })?;
            let copied = dest.copy_from_foreign(&page);
            dest.add_page(&copied, false).map_err(map_qpdf_error)?;
        }

        let mut writer = dest.writer();
        writer.preserve_encryption(false);
        writer.write_to_memory().map_err(map_qpdf_error)
    }

    /// Merge multiple PDFs into one
    ///
    /// # Arguments
    /// * `inputs` - Vector of raw PDF bytes to merge
    ///
    /// # Returns
    /// The merged PDF as bytes
    pub fn merge(inputs: &[&[u8]]) -> Result<Vec<u8>> {
        if inputs.is_empty() {
            return Err(Error::QpdfError {
                reason: "No input PDFs provided".to_string(),
            });
        }

        let dest = QPdf::empty();

        for (i, input_data) in inputs.iter().enumerate() {
            let source = QPdf::read_from_memory(input_data).map_err(|e| Error::QpdfError {
                reason: format!("Failed to read input PDF {}: {}", i, e),
            })?;

            let pages = source.get_pages().map_err(|e| Error::QpdfError {
                reason: format!("Failed to get pages from input PDF {}: {}", i, e),
            })?;

            for page in &pages {
                let copied = dest.copy_from_foreign(page);
                dest.add_page(&copied, false).map_err(map_qpdf_error)?;
            }
        }

        dest.writer().write_to_memory().map_err(map_qpdf_error)
    }

    /// Encrypt a PDF with password protection
    ///
    /// # Arguments
    /// * `input_data` - Raw PDF bytes
    /// * `user_password` - Password required to open the PDF
    /// * `owner_password` - Password required to change permissions (if None, same as user_password)
    /// * `allow_print` - Print permission: "full", "low", or "none"
    /// * `allow_copy` - Allow copying text/images
    /// * `allow_modify` - Allow modifying the document
    /// * `source_password` - Optional password if input PDF is already encrypted
    ///
    /// # Returns
    /// The encrypted PDF as bytes
    #[allow(clippy::too_many_arguments)]
    pub fn encrypt(
        input_data: &[u8],
        user_password: &str,
        owner_password: Option<&str>,
        allow_print: &str,
        allow_copy: bool,
        allow_modify: bool,
        source_password: Option<&str>,
    ) -> Result<Vec<u8>> {
        let qpdf = open_qpdf(input_data, source_password)?;

        let print_perm = match allow_print {
            "none" => PrintPermission::None,
            "low" => PrintPermission::Low,
            _ => PrintPermission::Full,
        };

        let owner_pwd = owner_password.unwrap_or(user_password);

        let encryption = EncryptionParams::R6(EncryptionParamsR6 {
            user_password: user_password.to_string(),
            owner_password: owner_pwd.to_string(),
            allow_accessibility: true,
            allow_extract: allow_copy,
            allow_assemble: allow_modify,
            allow_annotate_and_form: allow_modify,
            allow_form_filling: allow_modify,
            allow_modify_other: allow_modify,
            allow_print: print_perm,
            encrypt_metadata: true,
        });

        let mut writer = qpdf.writer();
        writer
            .preserve_encryption(false)
            .encryption_params(encryption);
        writer.write_to_memory().map_err(map_qpdf_error)
    }

    /// Decrypt a PDF (remove password protection)
    ///
    /// # Arguments
    /// * `input_data` - Raw PDF bytes
    /// * `password` - Password for the encrypted PDF
    ///
    /// # Returns
    /// The decrypted PDF as bytes
    pub fn decrypt(input_data: &[u8], password: &str) -> Result<Vec<u8>> {
        let qpdf =
            QPdf::read_from_memory_encrypted(input_data, password).map_err(map_qpdf_error)?;

        let mut writer = qpdf.writer();
        writer.preserve_encryption(false);
        writer.write_to_memory().map_err(map_qpdf_error)
    }

    /// Compress a PDF by optimizing streams and removing redundancy
    ///
    /// # Arguments
    /// * `input_data` - Raw PDF bytes
    /// * `password` - Optional password for encrypted PDFs
    /// * `object_streams` - Use object streams for smaller files (default: generate)
    /// * `_compression_level` - Ignored (kept for API compat; FFI uses built-in compression)
    ///
    /// # Returns
    /// The compressed PDF as bytes
    pub fn compress(
        input_data: &[u8],
        password: Option<&str>,
        object_streams: Option<&str>,
        _compression_level: Option<u8>,
    ) -> Result<Vec<u8>> {
        let qpdf = open_qpdf(input_data, password)?;

        let os_mode = match object_streams.unwrap_or("generate") {
            "preserve" => ObjectStreamMode::Preserve,
            "disable" => ObjectStreamMode::Disable,
            _ => ObjectStreamMode::Generate,
        };

        let mut writer = qpdf.writer();
        writer
            .object_stream_mode(os_mode)
            .compress_streams(true)
            .normalize_content(true)
            .preserve_unreferenced_objects(false)
            .preserve_encryption(false);
        writer.write_to_memory().map_err(map_qpdf_error)
    }

    /// Get the page count of a PDF
    ///
    /// # Arguments
    /// * `input_data` - Raw PDF bytes
    /// * `password` - Optional password for encrypted PDFs
    ///
    /// # Returns
    /// The number of pages in the PDF
    pub fn get_page_count(input_data: &[u8], password: Option<&str>) -> Result<u32> {
        let qpdf = open_qpdf(input_data, password)?;
        qpdf.get_num_pages().map_err(map_qpdf_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_page() {
        let result = parse_qpdf_page_range("3", 10).unwrap();
        assert_eq!(result, vec![2]); // 0-indexed
    }

    #[test]
    fn test_parse_range() {
        let result = parse_qpdf_page_range("1-3", 10).unwrap();
        assert_eq!(result, vec![0, 1, 2]);
    }

    #[test]
    fn test_parse_z_reference() {
        let result = parse_qpdf_page_range("z", 5).unwrap();
        assert_eq!(result, vec![4]); // last page, 0-indexed
    }

    #[test]
    fn test_parse_r_reference() {
        let result = parse_qpdf_page_range("r1", 5).unwrap();
        assert_eq!(result, vec![4]); // last page
        let result = parse_qpdf_page_range("r2", 5).unwrap();
        assert_eq!(result, vec![3]); // second to last
    }

    #[test]
    fn test_parse_reverse_range() {
        let result = parse_qpdf_page_range("3-1", 5).unwrap();
        assert_eq!(result, vec![2, 1, 0]); // 0-indexed, reversed
    }

    #[test]
    fn test_parse_z_range() {
        let result = parse_qpdf_page_range("z-1", 3).unwrap();
        assert_eq!(result, vec![2, 1, 0]); // all reversed
    }

    #[test]
    fn test_parse_odd_pages() {
        let result = parse_qpdf_page_range("1-6:odd", 10).unwrap();
        assert_eq!(result, vec![0, 2, 4]); // pages 1,3,5 -> 0-indexed
    }

    #[test]
    fn test_parse_even_pages() {
        let result = parse_qpdf_page_range("1-6:even", 10).unwrap();
        assert_eq!(result, vec![1, 3, 5]); // pages 2,4,6 -> 0-indexed
    }

    #[test]
    fn test_parse_comma_separated() {
        let result = parse_qpdf_page_range("1,3,5", 10).unwrap();
        assert_eq!(result, vec![0, 2, 4]);
    }

    #[test]
    fn test_parse_combined() {
        let result = parse_qpdf_page_range("1-3,5", 10).unwrap();
        assert_eq!(result, vec![0, 1, 2, 4]);
    }

    #[test]
    fn test_parse_invalid_page() {
        assert!(parse_qpdf_page_range("0", 5).is_err()); // 0 is invalid
        assert!(parse_qpdf_page_range("11", 10).is_err()); // out of range
        assert!(parse_qpdf_page_range("abc", 10).is_err()); // not a number
    }

    #[test]
    fn test_parse_empty() {
        assert!(parse_qpdf_page_range("", 10).is_err());
    }
}
