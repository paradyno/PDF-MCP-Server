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
}
