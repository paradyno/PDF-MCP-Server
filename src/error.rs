//! Error types for PDF MCP Server

use thiserror::Error;

/// Result type alias for PDF MCP Server
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for PDF MCP Server
#[derive(Error, Debug)]
pub enum Error {
    /// PDF file not found
    #[error("PDF not found: {path}")]
    PdfNotFound { path: String },

    /// Invalid PDF file
    #[error("Invalid PDF file: {reason}")]
    InvalidPdf { reason: String },

    /// PDF is password protected and no password was provided
    #[error("PDF is password protected")]
    PasswordRequired,

    /// Incorrect password provided
    #[error("Incorrect password")]
    IncorrectPassword,

    /// Invalid page range
    #[error("Invalid page range: {range}")]
    InvalidPageRange { range: String },

    /// Page out of bounds
    #[error("Page {page} out of bounds (total: {total})")]
    PageOutOfBounds { page: u32, total: u32 },

    /// Cache key not found
    #[error("Cache key not found: {key}")]
    CacheKeyNotFound { key: String },

    /// Source resolution error
    #[error("Failed to resolve source: {reason}")]
    SourceResolution { reason: String },

    /// Base64 decode error
    #[error("Invalid base64 data: {0}")]
    Base64Decode(#[from] base64::DecodeError),

    /// HTTP request error
    #[error("HTTP request failed: {0}")]
    HttpRequest(#[from] reqwest::Error),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// PDFium error
    #[error("PDFium error: {reason}")]
    Pdfium { reason: String },

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// qpdf error
    #[error("qpdf error: {reason}")]
    QpdfError { reason: String },

    /// Path access denied (outside allowed resource directories)
    #[error("Path access denied: {path}")]
    PathAccessDenied { path: String },

    /// SSRF blocked (URL resolves to private/reserved IP)
    #[error("SSRF blocked: {url}")]
    SsrfBlocked { url: String },

    /// Download too large
    #[error("Download too large: {size} bytes (max: {max_size} bytes)")]
    DownloadTooLarge { size: u64, max_size: u64 },

    /// Image dimension exceeded
    #[error("Image dimension exceeded: {detail}")]
    ImageDimensionExceeded { detail: String },
}

impl Error {
    /// Return a sanitized error message safe to send to clients.
    /// Internal details (paths, library errors, file sizes) are omitted.
    /// Full details should be logged via tracing before calling this.
    pub fn client_message(&self) -> String {
        match self {
            Error::PdfNotFound { .. } => "PDF not found".to_string(),
            Error::InvalidPdf { .. } => "Invalid PDF file".to_string(),
            Error::PasswordRequired => "PDF is password protected".to_string(),
            Error::IncorrectPassword => "Incorrect password".to_string(),
            Error::InvalidPageRange { range } => format!("Invalid page range: {}", range),
            Error::PageOutOfBounds { page, total } => {
                format!("Page {} out of bounds (total: {})", page, total)
            }
            Error::CacheKeyNotFound { .. } => "Cache key not found".to_string(),
            Error::SourceResolution { .. } => "Failed to resolve PDF source".to_string(),
            Error::Base64Decode(_) => "Invalid base64 data".to_string(),
            Error::HttpRequest(_) => "HTTP request failed".to_string(),
            Error::Io(_) => "I/O error".to_string(),
            Error::Pdfium { .. } => "PDF processing error".to_string(),
            Error::Serialization(_) => "Serialization error".to_string(),
            Error::QpdfError { .. } => "PDF processing error".to_string(),
            Error::PathAccessDenied { .. } => "Access denied".to_string(),
            Error::SsrfBlocked { .. } => "URL not allowed".to_string(),
            Error::DownloadTooLarge { max_size, .. } => {
                format!("Download exceeds maximum size of {} bytes", max_size)
            }
            Error::ImageDimensionExceeded { detail } => {
                format!("Image dimension exceeded: {}", detail)
            }
        }
    }
}
