//! qpdf CLI wrapper for PDF manipulation
//!
//! This module provides a wrapper around the qpdf command-line tool
//! for performing PDF page operations like splitting, merging, and rotating.

use crate::error::{Error, Result};
use std::process::Command;
use tempfile::NamedTempFile;

/// Wrapper for qpdf CLI operations
pub struct QpdfWrapper;

impl QpdfWrapper {
    /// Check if qpdf is available on the system
    pub fn check_available() -> Result<()> {
        let output = Command::new("qpdf")
            .arg("--version")
            .output()
            .map_err(|_| Error::QpdfNotFound)?;

        if output.status.success() {
            Ok(())
        } else {
            Err(Error::QpdfNotFound)
        }
    }

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
        // Create temporary files for input and output
        let input_file = NamedTempFile::new()?;
        let output_file = NamedTempFile::new()?;

        // Write input data to temporary file
        std::fs::write(input_file.path(), input_data)?;

        // Build qpdf command
        let mut cmd = Command::new("qpdf");

        // Add password if provided
        if let Some(pwd) = password {
            cmd.arg(format!("--password={}", pwd));
        }

        // Input file and page selection
        cmd.arg(input_file.path());
        cmd.arg("--pages");
        cmd.arg("."); // "." refers to the input file
        cmd.arg(pages);
        cmd.arg("--");
        // Decrypt output to make it easier to work with
        cmd.arg("--decrypt");
        cmd.arg(output_file.path());

        // Execute qpdf
        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Check for common error patterns
            if stderr.contains("invalid password") || stderr.contains("password") {
                return Err(Error::IncorrectPassword);
            }
            if stderr.contains("invalid page range") || stderr.contains("page range") {
                return Err(Error::InvalidPageRange {
                    range: pages.to_string(),
                });
            }
            return Err(Error::QpdfError {
                reason: stderr.to_string(),
            });
        }

        // Read output file
        let result = std::fs::read(output_file.path())?;

        Ok(result)
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

        // Create temporary files for all inputs and output
        let mut input_files: Vec<NamedTempFile> = Vec::new();
        for (i, input_data) in inputs.iter().enumerate() {
            let input_file = NamedTempFile::new()?;
            std::fs::write(input_file.path(), input_data).map_err(|e| Error::QpdfError {
                reason: format!("Failed to write input file {}: {}", i, e),
            })?;
            input_files.push(input_file);
        }

        let output_file = NamedTempFile::new()?;

        // Build qpdf command: qpdf --empty --pages file1.pdf file2.pdf ... -- output.pdf
        let mut cmd = Command::new("qpdf");
        cmd.arg("--empty");
        cmd.arg("--pages");

        // Add all input files with full page range
        for input_file in &input_files {
            cmd.arg(input_file.path());
            cmd.arg("1-z"); // All pages from each file
        }

        cmd.arg("--");
        cmd.arg(output_file.path());

        // Execute qpdf
        let output = cmd.output()?;

        // Check for success or warning (qpdf may return non-zero for warnings)
        // qpdf outputs warnings to stderr
        let stderr = String::from_utf8_lossy(&output.stderr);

        // If command failed, check if it was actually a success with warnings
        if !output.status.success() {
            // qpdf says "operation succeeded with warnings" in stderr when it completes with warnings
            if !stderr.contains("operation succeeded") {
                return Err(Error::QpdfError {
                    reason: stderr.to_string(),
                });
            }
        }

        // Read output file
        let result = std::fs::read(output_file.path())?;

        Ok(result)
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
        // Create temporary files for input and output
        let input_file = NamedTempFile::new()?;
        let output_file = NamedTempFile::new()?;

        // Write input data to temporary file
        std::fs::write(input_file.path(), input_data)?;

        // Build qpdf command
        let mut cmd = Command::new("qpdf");

        // Add source password if provided
        if let Some(pwd) = source_password {
            cmd.arg(format!("--password={}", pwd));
        }

        cmd.arg(input_file.path());

        // Encryption settings: --encrypt user-pass owner-pass 256 --
        let owner_pwd = owner_password.unwrap_or(user_password);
        cmd.arg("--encrypt");
        cmd.arg(user_password);
        cmd.arg(owner_pwd);
        cmd.arg("256"); // 256-bit AES encryption

        // Print permission
        match allow_print {
            "none" => {
                cmd.arg("--print=none");
            }
            "low" => {
                cmd.arg("--print=low");
            }
            _ => {
                cmd.arg("--print=full");
            }
        }

        // Copy permission
        if !allow_copy {
            cmd.arg("--extract=n");
        }

        // Modify permission
        if !allow_modify {
            cmd.arg("--modify=none");
        }

        cmd.arg("--"); // End of encryption options
        cmd.arg(output_file.path());

        // Execute qpdf
        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("invalid password") || stderr.contains("password") {
                return Err(Error::IncorrectPassword);
            }
            return Err(Error::QpdfError {
                reason: stderr.to_string(),
            });
        }

        // Read output file
        let result = std::fs::read(output_file.path())?;

        Ok(result)
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
        // Create temporary files for input and output
        let input_file = NamedTempFile::new()?;
        let output_file = NamedTempFile::new()?;

        // Write input data to temporary file
        std::fs::write(input_file.path(), input_data)?;

        // Build qpdf command: qpdf --password=secret --decrypt input.pdf output.pdf
        let mut cmd = Command::new("qpdf");
        cmd.arg(format!("--password={}", password));
        cmd.arg("--decrypt");
        cmd.arg(input_file.path());
        cmd.arg(output_file.path());

        // Execute qpdf
        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("invalid password") || stderr.contains("password") {
                return Err(Error::IncorrectPassword);
            }
            return Err(Error::QpdfError {
                reason: stderr.to_string(),
            });
        }

        // Read output file
        let result = std::fs::read(output_file.path())?;

        Ok(result)
    }

    /// Compress a PDF by optimizing streams and removing redundancy
    ///
    /// # Arguments
    /// * `input_data` - Raw PDF bytes
    /// * `password` - Optional password for encrypted PDFs
    /// * `object_streams` - Use object streams for smaller files (default: generate)
    /// * `compression_level` - Stream compression level (1-9, default: 9)
    ///
    /// # Returns
    /// The compressed PDF as bytes
    pub fn compress(
        input_data: &[u8],
        password: Option<&str>,
        object_streams: Option<&str>,
        compression_level: Option<u8>,
    ) -> Result<Vec<u8>> {
        // Create temporary files for input and output
        let input_file = NamedTempFile::new()?;
        let output_file = NamedTempFile::new()?;

        // Write input data to temporary file
        std::fs::write(input_file.path(), input_data)?;

        // Build qpdf command
        let mut cmd = Command::new("qpdf");

        // Add password if provided
        if let Some(pwd) = password {
            cmd.arg(format!("--password={}", pwd));
        }

        cmd.arg(input_file.path());

        // Object streams optimization
        match object_streams.unwrap_or("generate") {
            "generate" => {
                cmd.arg("--object-streams=generate");
            }
            "preserve" => {
                cmd.arg("--object-streams=preserve");
            }
            "disable" => {
                cmd.arg("--object-streams=disable");
            }
            _ => {
                cmd.arg("--object-streams=generate");
            }
        }

        // Stream data compression
        cmd.arg("--recompress-flate");

        // Compression level (1-9)
        let level = compression_level.unwrap_or(9).clamp(1, 9);
        cmd.arg(format!("--compression-level={}", level));

        // Stream optimization
        cmd.arg("--optimize-images");

        // Remove unreferenced objects
        cmd.arg("--remove-unreferenced-resources=yes");

        // Normalize content streams
        cmd.arg("--normalize-content=y");

        // Decrypt output if input was encrypted
        cmd.arg("--decrypt");

        cmd.arg(output_file.path());

        // Execute qpdf
        let output = cmd.output()?;

        // Check for success or warning
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            if stderr.contains("invalid password") || stderr.contains("password") {
                return Err(Error::IncorrectPassword);
            }
            // qpdf may succeed with warnings
            if !stderr.contains("operation succeeded") {
                return Err(Error::QpdfError {
                    reason: stderr.to_string(),
                });
            }
        }

        // Read output file
        let result = std::fs::read(output_file.path())?;

        Ok(result)
    }

    /// Get the page count of a PDF using qpdf
    ///
    /// # Arguments
    /// * `input_data` - Raw PDF bytes
    /// * `password` - Optional password for encrypted PDFs
    ///
    /// # Returns
    /// The number of pages in the PDF
    pub fn get_page_count(input_data: &[u8], password: Option<&str>) -> Result<u32> {
        // Create temporary file for input
        let input_file = NamedTempFile::new()?;
        std::fs::write(input_file.path(), input_data)?;

        // Build qpdf command
        let mut cmd = Command::new("qpdf");

        // Add password if provided
        if let Some(pwd) = password {
            cmd.arg(format!("--password={}", pwd));
        }

        cmd.arg("--show-npages");
        cmd.arg(input_file.path());

        // Execute qpdf
        let output = cmd.output()?;

        // Check for success or warning
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            if stderr.contains("invalid password") || stderr.contains("password") {
                return Err(Error::IncorrectPassword);
            }
            // qpdf may succeed with warnings
            if !stderr.contains("operation succeeded") {
                return Err(Error::QpdfError {
                    reason: stderr.to_string(),
                });
            }
        }

        // Parse page count from stdout
        let stdout = String::from_utf8_lossy(&output.stdout);
        let page_count: u32 = stdout.trim().parse().map_err(|_| Error::QpdfError {
            reason: format!("Failed to parse page count: {}", stdout),
        })?;

        Ok(page_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qpdf_available() {
        // This test requires qpdf to be installed
        let result = QpdfWrapper::check_available();
        // In CI/Docker environment, qpdf should be available
        // If running locally without qpdf, this test will fail
        assert!(result.is_ok(), "qpdf should be available");
    }
}
