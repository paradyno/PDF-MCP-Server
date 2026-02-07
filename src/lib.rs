//! PDF MCP Server Library
//!
//! This crate provides MCP tools for PDF processing:
//! - `extract_text`: Extract text content from PDFs
//! - `extract_outline`: Extract PDF bookmarks/table of contents
//! - `search`: Search for text within PDFs
//! - `list_pdfs`: List PDF files in a directory

pub mod error;
pub mod pdf;
pub mod server;
pub mod source;

pub use error::{Error, Result};
pub use server::{
    run_server, run_server_with_config, run_server_with_dirs, ListPdfsParams, ListPdfsResult,
    PdfFileInfo, PdfServer, PdfSource, ServerConfig,
};
