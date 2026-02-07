//! MCP Server implementation using rmcp

use crate::pdf::{
    extract_annotations, extract_form_fields, extract_images_from_pages, extract_links,
    extract_text_with_options, fill_form_fields, get_page_info, parse_page_range,
    render_pages_to_images, PdfReader, QpdfWrapper, TextExtractionConfig,
};
use crate::source::{
    resolve_base64, resolve_cache, resolve_path, resolve_url, CacheManager, ResolvedPdf,
};
use anyhow::Result;
use rmcp::{
    handler::server::tool::ToolRouter, handler::server::wrapper::Parameters, model::*,
    schemars::JsonSchema, service::RequestContext, tool, tool_handler, tool_router, RoleServer,
    ServerHandler, ServiceExt,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// PDF source specification
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum PdfSource {
    /// File path (absolute or relative)
    Path {
        /// Path to the PDF file
        path: String,
    },
    /// Base64 encoded PDF data
    Base64 {
        /// Base64 encoded PDF content
        base64: String,
    },
    /// URL to download PDF from
    Url {
        /// URL of the PDF file
        url: String,
    },
    /// Reference to cached PDF
    CacheRef {
        /// Cache key from previous operation
        cache_key: String,
    },
}

impl<'de> serde::Deserialize<'de> for PdfSource {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        if let Some(obj) = value.as_object() {
            if let Some(v) = obj.get("path") {
                if let Some(s) = v.as_str() {
                    return Ok(PdfSource::Path {
                        path: s.to_string(),
                    });
                }
                return Err(serde::de::Error::custom(
                    "\"path\" must be a string",
                ));
            }
            if let Some(v) = obj.get("base64") {
                if let Some(s) = v.as_str() {
                    return Ok(PdfSource::Base64 {
                        base64: s.to_string(),
                    });
                }
                return Err(serde::de::Error::custom(
                    "\"base64\" must be a string",
                ));
            }
            if let Some(v) = obj.get("url") {
                if let Some(s) = v.as_str() {
                    return Ok(PdfSource::Url {
                        url: s.to_string(),
                    });
                }
                return Err(serde::de::Error::custom(
                    "\"url\" must be a string",
                ));
            }
            if let Some(v) = obj.get("cache_key") {
                if let Some(s) = v.as_str() {
                    return Ok(PdfSource::CacheRef {
                        cache_key: s.to_string(),
                    });
                }
                return Err(serde::de::Error::custom(
                    "\"cache_key\" must be a string",
                ));
            }
            let keys: Vec<&String> = obj.keys().collect();
            Err(serde::de::Error::custom(format!(
                "Invalid source: expected an object with one of \"path\", \"base64\", \"url\", or \"cache_key\", but got keys: {:?}",
                keys
            )))
        } else {
            Err(serde::de::Error::custom(format!(
                "Invalid source: expected an object with one of \"path\", \"base64\", \"url\", or \"cache_key\", but got {}",
                match &value {
                    serde_json::Value::Array(_) => "an array",
                    serde_json::Value::String(_) => "a string",
                    serde_json::Value::Number(_) => "a number",
                    serde_json::Value::Bool(_) => "a boolean",
                    serde_json::Value::Null => "null",
                    _ => "unknown type",
                }
            )))
        }
    }
}

/// Security and resource configuration for the PDF MCP Server
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Directories to expose as PDF resources
    pub resource_dirs: Vec<String>,
    /// Allow URLs that resolve to private/reserved IPs (default: false)
    pub allow_private_urls: bool,
    /// Maximum download size in bytes for URL sources (default: 100MB)
    pub max_download_bytes: u64,
    /// Maximum total bytes in cache (default: 512MB)
    pub cache_max_bytes: usize,
    /// Maximum number of cache entries (default: 100)
    pub cache_max_entries: usize,
    /// Maximum image scale factor for convert_page_to_image (default: 10.0)
    pub max_image_scale: f32,
    /// Maximum total pixel area for convert_page_to_image (default: 100_000_000)
    pub max_image_pixels: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            resource_dirs: Vec::new(),
            allow_private_urls: false,
            max_download_bytes: 100 * 1024 * 1024, // 100MB
            cache_max_bytes: 512 * 1024 * 1024,    // 512MB
            cache_max_entries: 100,
            max_image_scale: 10.0,
            max_image_pixels: 100_000_000,
        }
    }
}

/// PDF MCP Server
#[derive(Clone)]
pub struct PdfServer {
    cache: Arc<RwLock<CacheManager>>,
    tool_router: ToolRouter<Self>,
    /// Server configuration
    config: Arc<ServerConfig>,
}

// ============================================================================
// Request/Response types for list_pdfs
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListPdfsParams {
    /// Directory to search for PDF files
    pub directory: String,
    /// Search subdirectories recursively (default: false)
    #[serde(default)]
    pub recursive: bool,
    /// Filename pattern to filter (e.g., "report*.pdf"). Supports glob patterns.
    #[serde(default)]
    pub pattern: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PdfFileInfo {
    /// Full path to the PDF file
    pub path: String,
    /// Filename only
    pub name: String,
    /// File size in bytes
    pub size: u64,
    /// Last modified time (ISO 8601 format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListPdfsResult {
    /// Directory that was searched
    pub directory: String,
    /// List of PDF files found
    pub files: Vec<PdfFileInfo>,
    /// Total number of files found
    pub total_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for extract_text
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExtractTextParams {
    /// PDF sources to process
    pub sources: Vec<PdfSource>,
    /// Page selection (e.g., "1-5,10,15-20")
    #[serde(default)]
    pub pages: Option<String>,
    /// Include PDF metadata
    #[serde(default = "default_true")]
    pub include_metadata: bool,
    /// Include extracted images (base64 encoded PNG)
    #[serde(default)]
    pub include_images: bool,
    /// Password for encrypted PDFs
    #[serde(default)]
    pub password: Option<String>,
    /// Enable caching
    #[serde(default)]
    pub cache: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PdfMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub creation_date: Option<String>,
    pub modification_date: Option<String>,
    pub page_count: u32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PageContent {
    pub page: u32,
    pub text: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ImageInfo {
    /// Page number (1-indexed)
    pub page: u32,
    /// Image index on the page
    pub index: u32,
    /// Image width in pixels
    pub width: u32,
    /// Image height in pixels
    pub height: u32,
    /// Base64 encoded image data (PNG format)
    pub data_base64: String,
    /// MIME type (always "image/png")
    pub mime_type: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExtractTextResult {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<PdfMetadata>,
    pub pages: Vec<PageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<ImageInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for extract_metadata
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExtractMetadataParams {
    /// PDF sources to process
    pub sources: Vec<PdfSource>,
    /// Password for encrypted PDFs
    #[serde(default)]
    pub password: Option<String>,
    /// Enable caching
    #[serde(default)]
    pub cache: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExtractMetadataResult {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modification_date: Option<String>,
    pub page_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for extract_outline
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExtractOutlineParams {
    /// PDF sources to process
    pub sources: Vec<PdfSource>,
    /// Password for encrypted PDFs
    #[serde(default)]
    pub password: Option<String>,
    /// Enable caching
    #[serde(default)]
    pub cache: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct OutlineEntry {
    /// Title of the bookmark
    pub title: String,
    /// Destination page number (1-indexed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    /// Child entries
    pub children: Vec<OutlineEntry>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExtractOutlineResult {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    pub outline: Vec<OutlineEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for search
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// PDF sources to search in
    pub sources: Vec<PdfSource>,
    /// Search query
    pub query: String,
    /// Case-sensitive search
    #[serde(default)]
    pub case_sensitive: bool,
    /// Maximum number of results
    #[serde(default = "default_max_results")]
    pub max_results: u32,
    /// Characters of context around each match
    #[serde(default = "default_context_chars")]
    pub context_chars: u32,
    /// Password for encrypted PDFs
    #[serde(default)]
    pub password: Option<String>,
    /// Enable caching
    #[serde(default)]
    pub cache: bool,
}

fn default_max_results() -> u32 {
    100
}

fn default_context_chars() -> u32 {
    50
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchMatch {
    /// Page number (1-indexed)
    pub page: u32,
    /// Matched text with context
    pub context: String,
    /// Position in text (character offset)
    pub position: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchResult {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    pub matches: Vec<SearchMatch>,
    pub total_matches: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for extract_annotations
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExtractAnnotationsParams {
    /// PDF sources to process
    pub sources: Vec<PdfSource>,
    /// Filter by annotation types (empty = all types).
    /// Supported types: highlight, underline, strikeout, squiggly, text, freetext, link, stamp, ink, etc.
    #[serde(default)]
    pub annotation_types: Vec<String>,
    /// Include only annotations from specific pages (e.g., "1-5,10")
    #[serde(default)]
    pub pages: Option<String>,
    /// Password for encrypted PDFs
    #[serde(default)]
    pub password: Option<String>,
    /// Enable caching
    #[serde(default)]
    pub cache: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RectInfo {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AnnotationInfo {
    /// Page number (1-indexed)
    pub page: u32,
    /// Annotation type (highlight, underline, text, etc.)
    pub annotation_type: String,
    /// Text content (comment, note)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contents: Option<String>,
    /// Author name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Creation date
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// Modification date
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    /// Bounding rectangle
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<RectInfo>,
    /// Highlighted/underlined text (extracted from page)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlighted_text: Option<String>,
    /// Fill color (hex format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExtractAnnotationsResult {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    pub annotations: Vec<AnnotationInfo>,
    pub total_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for split_pdf
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SplitPdfParams {
    /// Source PDF to split
    pub source: PdfSource,
    /// Pages to extract using qpdf page range syntax.
    ///
    /// Basic syntax:
    /// - "1-5" : pages 1 through 5
    /// - "1,3,5" : specific pages 1, 3, and 5
    /// - "1-5,10,15-20" : combined ranges
    ///
    /// Special references:
    /// - "z" : last page (e.g., "5-z" for page 5 to end)
    /// - "r1" : last page, "r2" : second to last, etc.
    /// - "z-1" or "r1-1" : all pages in reverse order
    ///
    /// Modifiers (append to a range):
    /// - ":odd" : odd pages from the range (e.g., "1-z:odd" for all odd pages)
    /// - ":even" : even pages from the range (e.g., "1-z:even" for all even pages)
    ///
    /// Exclusions:
    /// - "1-10,x5" : pages 1-10 except page 5
    /// - "1-z,x5-10" : all pages except 5-10
    ///
    /// Duplicates allowed:
    /// - "1,1,1" : page 1 three times
    pub pages: String,
    /// Output file path (optional). If provided, saves PDF to this path.
    /// Supports both absolute and relative paths.
    #[serde(default)]
    pub output_path: Option<String>,
    /// Password for encrypted PDFs
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SplitPdfResult {
    /// Source identifier
    pub source: String,
    /// Cache key for the output PDF (always provided for chaining operations)
    pub output_cache_key: String,
    /// Number of pages in output PDF
    pub output_page_count: u32,
    /// Path where PDF was saved (if output_path was specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for merge_pdfs
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MergePdfsParams {
    /// PDF sources to merge (in order). All PDFs will be combined into a single output PDF.
    pub sources: Vec<PdfSource>,
    /// Output file path (optional). If provided, saves the merged PDF to this path.
    /// Supports both absolute and relative paths.
    #[serde(default)]
    pub output_path: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MergePdfsResult {
    /// Number of source PDFs merged
    pub source_count: u32,
    /// Cache key for the output PDF (always provided for chaining operations)
    pub output_cache_key: String,
    /// Total pages in output PDF
    pub output_page_count: u32,
    /// Path where PDF was saved (if output_path was specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for protect_pdf
// ============================================================================

fn default_full() -> String {
    "full".to_string()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProtectPdfParams {
    /// Source PDF to protect
    pub source: PdfSource,
    /// User password (required to open the PDF)
    pub user_password: String,
    /// Owner password (required to change permissions). If not set, same as user_password.
    #[serde(default)]
    pub owner_password: Option<String>,
    /// Allow printing: "full" (default), "low" (low resolution), or "none"
    #[serde(default = "default_full")]
    pub allow_print: String,
    /// Allow copying text/images (default: true)
    #[serde(default = "default_true")]
    pub allow_copy: bool,
    /// Allow modifying the document (default: true)
    #[serde(default = "default_true")]
    pub allow_modify: bool,
    /// Output file path (optional). If provided, saves the protected PDF to this path.
    #[serde(default)]
    pub output_path: Option<String>,
    /// Password for source PDF (if already encrypted)
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProtectPdfResult {
    /// Source identifier
    pub source: String,
    /// Cache key for the output PDF (always provided for chaining operations)
    pub output_cache_key: String,
    /// Number of pages in output PDF
    pub output_page_count: u32,
    /// Path where PDF was saved (if output_path was specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for unprotect_pdf
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UnprotectPdfParams {
    /// Source PDF to unprotect
    pub source: PdfSource,
    /// Password for the encrypted PDF
    pub password: String,
    /// Output file path (optional). If provided, saves the unprotected PDF to this path.
    #[serde(default)]
    pub output_path: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct UnprotectPdfResult {
    /// Source identifier
    pub source: String,
    /// Cache key for the output PDF (always provided for chaining operations)
    pub output_cache_key: String,
    /// Number of pages in output PDF
    pub output_page_count: u32,
    /// Path where PDF was saved (if output_path was specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for extract_links
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExtractLinksParams {
    /// PDF sources to process
    pub sources: Vec<PdfSource>,
    /// Include only links from specific pages (e.g., "1-5,10")
    #[serde(default)]
    pub pages: Option<String>,
    /// Password for encrypted PDFs
    #[serde(default)]
    pub password: Option<String>,
    /// Enable caching
    #[serde(default)]
    pub cache: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkInfo {
    /// Page number (1-indexed)
    pub page: u32,
    /// URL for external links
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Destination page for internal links
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dest_page: Option<u32>,
    /// Bounding rectangle
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<RectInfo>,
    /// Link text (if extractable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExtractLinksResult {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    pub links: Vec<LinkInfo>,
    pub total_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for get_page_info
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetPageInfoParams {
    /// PDF sources to process
    pub sources: Vec<PdfSource>,
    /// Password for encrypted PDFs
    #[serde(default)]
    pub password: Option<String>,
    /// Enable caching
    #[serde(default)]
    pub cache: bool,
    /// Skip calculating file sizes for each page (faster but less info).
    /// By default, file sizes are calculated by splitting each page (~16ms/page).
    /// Set to true to skip this calculation for better performance.
    #[serde(default)]
    pub skip_file_sizes: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PageInfo {
    /// Page number (1-indexed)
    pub page: u32,
    /// Page width in points (1 point = 1/72 inch)
    pub width: f32,
    /// Page height in points
    pub height: f32,
    /// Page rotation in degrees (0, 90, 180, 270)
    pub rotation: i32,
    /// Page orientation: "portrait", "landscape", or "square"
    pub orientation: String,
    /// Text content character count
    pub char_count: usize,
    /// Word count (whitespace-separated)
    pub word_count: usize,
    /// Estimated token count for LLMs.
    ///
    /// NOTE: This is a model-dependent approximation. Actual token counts vary:
    /// - Latin/English: ~4 characters per token
    /// - CJK (Chinese/Japanese/Korean): ~2 tokens per character
    ///
    /// Use as rough guidance for context window planning only.
    pub estimated_token_count: usize,
    /// Actual file size in bytes when this page is extracted as standalone PDF.
    /// Calculated by default. Use skip_file_sizes=true to disable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GetPageInfoResult {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    pub pages: Vec<PageInfo>,
    pub total_pages: u32,
    /// Total character count across all pages
    pub total_chars: usize,
    /// Total word count across all pages
    pub total_words: usize,
    /// Total estimated tokens across all pages.
    /// NOTE: Token estimation is model-dependent. See page-level estimated_token_count for details.
    pub total_estimated_token_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for compress_pdf
// ============================================================================

fn default_compression_level() -> u8 {
    9
}

fn default_object_streams() -> String {
    "generate".to_string()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompressPdfParams {
    /// Source PDF to compress
    pub source: PdfSource,
    /// Object streams mode: "generate" (best compression), "preserve", or "disable"
    #[serde(default = "default_object_streams")]
    pub object_streams: String,
    /// Compression level (1-9, higher = better compression but slower)
    #[serde(default = "default_compression_level")]
    pub compression_level: u8,
    /// Output file path (optional). If provided, saves the compressed PDF to this path.
    #[serde(default)]
    pub output_path: Option<String>,
    /// Password for source PDF (if encrypted)
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CompressPdfResult {
    /// Source identifier
    pub source: String,
    /// Cache key for the output PDF (always provided for chaining operations)
    pub output_cache_key: String,
    /// Original file size in bytes
    pub original_size: usize,
    /// Compressed file size in bytes
    pub compressed_size: usize,
    /// Compression ratio (compressed/original, lower is better)
    pub compression_ratio: f32,
    /// Bytes saved
    pub bytes_saved: i64,
    /// Number of pages in output PDF
    pub output_page_count: u32,
    /// Path where PDF was saved (if output_path was specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for convert_page_to_image
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConvertPageToImageParams {
    /// PDF sources to process
    pub sources: Vec<PdfSource>,
    /// Page selection (e.g., "1-3,5"). Defaults to all pages.
    #[serde(default)]
    pub pages: Option<String>,
    /// Target image width in pixels (default: 1200). Ignored if scale is set.
    #[serde(default)]
    pub width: Option<u16>,
    /// Target image height in pixels. Ignored if scale is set.
    #[serde(default)]
    pub height: Option<u16>,
    /// Scale factor relative to PDF page size (e.g., 2.0 for double size). Overrides width/height.
    #[serde(default)]
    pub scale: Option<f32>,
    /// Password for encrypted PDFs
    #[serde(default)]
    pub password: Option<String>,
    /// Enable caching
    #[serde(default)]
    pub cache: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RenderedPageInfo {
    /// Page number (1-indexed)
    pub page: u32,
    /// Image width in pixels
    pub width: u32,
    /// Image height in pixels
    pub height: u32,
    /// Base64-encoded PNG image data
    pub data_base64: String,
    /// MIME type (always "image/png")
    pub mime_type: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ConvertPageToImageResult {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    pub pages: Vec<RenderedPageInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for extract_form_fields
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExtractFormFieldsParams {
    /// PDF sources to process
    pub sources: Vec<PdfSource>,
    /// Page selection (e.g., "1-3,5"). Defaults to all pages.
    #[serde(default)]
    pub pages: Option<String>,
    /// Password for encrypted PDFs
    #[serde(default)]
    pub password: Option<String>,
    /// Enable caching
    #[serde(default)]
    pub cache: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FormFieldOptionInfoResponse {
    /// Display label
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Whether this option is currently selected
    pub is_selected: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FormFieldPropertiesResponse {
    /// Whether text field is multiline
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_multiline: Option<bool>,
    /// Whether text field is a password field
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_password: Option<bool>,
    /// Whether combo box is editable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_editable: Option<bool>,
    /// Whether combo/list box allows multiple selections
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_multiselect: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FormFieldInfoResponse {
    /// Page number (1-indexed)
    pub page: u32,
    /// Field name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Field type (text, checkbox, radio_button, combo_box, list_box, push_button, signature, unknown)
    pub field_type: String,
    /// Current value (for text fields)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Whether checked (for checkbox/radio)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_checked: Option<bool>,
    /// Read-only flag
    pub is_read_only: bool,
    /// Required flag
    pub is_required: bool,
    /// Available options (for combo_box/list_box)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<FormFieldOptionInfoResponse>>,
    /// Type-specific properties
    pub properties: FormFieldPropertiesResponse,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExtractFormFieldsResult {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    pub fields: Vec<FormFieldInfoResponse>,
    pub total_fields: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for fill_form
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FormFieldValueParam {
    /// Field name to match
    pub name: String,
    /// Text value (for text fields)
    #[serde(default)]
    pub value: Option<String>,
    /// Checked state (for checkbox/radio)
    #[serde(default)]
    pub checked: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FillFormParams {
    /// Source PDF containing form fields
    pub source: PdfSource,
    /// Field values to set
    pub field_values: Vec<FormFieldValueParam>,
    /// Output file path (optional). If provided, saves the filled PDF to this path.
    #[serde(default)]
    pub output_path: Option<String>,
    /// Password for encrypted PDFs
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SkippedFieldInfo {
    /// Field name
    pub name: String,
    /// Reason the field was skipped
    pub reason: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FillFormResult {
    /// Source identifier
    pub source: String,
    /// Cache key for the output PDF (always provided for chaining operations)
    pub output_cache_key: String,
    /// Number of fields successfully filled
    pub fields_filled: u32,
    /// Fields that could not be filled
    pub fields_skipped: Vec<SkippedFieldInfo>,
    /// Number of pages in output PDF
    pub output_page_count: u32,
    /// Path where PDF was saved (if output_path was specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Request/Response types for summarize_structure
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SummarizeStructureParams {
    /// PDF sources to process
    pub sources: Vec<PdfSource>,
    /// Password for encrypted PDFs
    #[serde(default)]
    pub password: Option<String>,
    /// Enable caching
    #[serde(default)]
    pub cache: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PageSummary {
    /// Page number (1-indexed)
    pub page: u32,
    /// Page width in points
    pub width: f32,
    /// Page height in points
    pub height: f32,
    /// Character count
    pub char_count: usize,
    /// Word count
    pub word_count: usize,
    /// Whether page has images
    pub has_images: bool,
    /// Whether page has links
    pub has_links: bool,
    /// Whether page has annotations
    pub has_annotations: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SummarizeStructureResult {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    /// Total page count
    pub page_count: u32,
    /// File size in bytes
    pub file_size: usize,
    /// PDF metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<PdfMetadata>,
    /// Whether the PDF has an outline (bookmarks)
    pub has_outline: bool,
    /// Number of outline/bookmark entries
    pub outline_items: u32,
    /// Total characters across all pages
    pub total_chars: usize,
    /// Total words across all pages
    pub total_words: usize,
    /// Total estimated tokens for LLMs
    pub total_estimated_tokens: usize,
    /// Per-page summary
    pub pages: Vec<PageSummary>,
    /// Total images across all pages
    pub total_images: u32,
    /// Total links across all pages
    pub total_links: u32,
    /// Total annotations across all pages
    pub total_annotations: u32,
    /// Whether the PDF has form fields
    pub has_form: bool,
    /// Total form field count
    pub form_field_count: u32,
    /// Form field types and counts (e.g., {"text": 5, "checkbox": 2})
    pub form_field_types: HashMap<String, u32>,
    /// Whether the PDF was encrypted (password was needed)
    pub is_encrypted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Tool implementations
// ============================================================================

#[tool_router]
impl PdfServer {
    pub fn new() -> Self {
        Self::with_config(ServerConfig::default())
    }

    /// Create a new PdfServer with specified resource directories
    pub fn with_resource_dirs(dirs: Vec<String>) -> Self {
        Self::with_config(ServerConfig {
            resource_dirs: dirs,
            ..ServerConfig::default()
        })
    }

    /// Create a new PdfServer with full configuration
    pub fn with_config(config: ServerConfig) -> Self {
        let cache = CacheManager::new(config.cache_max_entries, config.cache_max_bytes);
        Self {
            cache: Arc::new(RwLock::new(cache)),
            tool_router: Self::tool_router(),
            config: Arc::new(config),
        }
    }

    /// Extract text content from PDF files
    #[tool(
        description = "Extract text content from PDF files. Supports page selection, metadata extraction, and batch processing of multiple PDFs.

Source format: each element must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn extract_text(&self, Parameters(params): Parameters<ExtractTextParams>) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_extract_text(source, &params)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "extract_text failed");
                    ExtractTextResult {
                        source: Self::source_name(source),
                        cache_key: None,
                        metadata: None,
                        pages: vec![],
                        images: None,
                        error: Some(e.client_message()),
                    }
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Extract PDF bookmarks/table of contents
    #[tool(
        description = "Extract PDF bookmarks/table of contents with page numbers and hierarchy.

Source format: each element must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn extract_outline(
        &self,
        Parameters(params): Parameters<ExtractOutlineParams>,
    ) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_extract_outline(source, &params)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "extract_outline failed");
                    ExtractOutlineResult {
                        source: Self::source_name(source),
                        cache_key: None,
                        outline: vec![],
                        error: Some(e.client_message()),
                    }
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Search for text within PDF files
    #[tool(
        description = "Search for text within PDF files. Returns matching text with context and page locations.

Source format: each element must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn search(&self, Parameters(params): Parameters<SearchParams>) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_search(source, &params)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "search failed");
                    SearchResult {
                        source: Self::source_name(source),
                        cache_key: None,
                        matches: vec![],
                        total_matches: 0,
                        error: Some(e.client_message()),
                    }
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Extract PDF metadata without loading full content
    #[tool(
        description = "Extract PDF metadata (author, title, creation date, page count, etc.) without loading full content. Fast operation for getting document information.

Source format: each element must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn extract_metadata(
        &self,
        Parameters(params): Parameters<ExtractMetadataParams>,
    ) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_extract_metadata(source, &params)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "extract_metadata failed");
                    ExtractMetadataResult {
                        source: Self::source_name(source),
                        cache_key: None,
                        title: None,
                        author: None,
                        subject: None,
                        creator: None,
                        producer: None,
                        creation_date: None,
                        modification_date: None,
                        page_count: 0,
                        error: Some(e.client_message()),
                    }
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Extract annotations from PDF files
    #[tool(
        description = "Extract annotations (highlights, comments, underlines, etc.) from PDF files. Returns annotation content, author, dates, and highlighted text.

Source format: each element must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn extract_annotations(
        &self,
        Parameters(params): Parameters<ExtractAnnotationsParams>,
    ) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_extract_annotations(source, &params)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "extract_annotations failed");
                    ExtractAnnotationsResult {
                        source: Self::source_name(source),
                        cache_key: None,
                        annotations: vec![],
                        total_count: 0,
                        error: Some(e.client_message()),
                    }
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Split PDF by extracting specific pages
    #[tool(
        description = "Extract specific pages from a PDF to create a new PDF. The output is always cached (output_cache_key) for chaining with other tools like extract_text.

Page range syntax:
- Basic: \"1-5\" (range), \"1,3,5\" (specific pages), \"1-5,10,15-20\" (combined)
- Last page: \"z\" or \"r1\" (e.g., \"5-z\" for page 5 to end)
- Reverse: \"z-1\" (all pages reversed), \"5-1\" (pages 5 to 1)
- Odd/even: \"1-z:odd\" (all odd pages), \"1-z:even\" (all even pages)
- Exclude: \"1-10,x5\" (1-10 except 5), \"1-z,x5-10\" (all except 5-10)
- Duplicate: \"1,1,1\" (page 1 three times)

Source format: must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn split_pdf(&self, Parameters(params): Parameters<SplitPdfParams>) -> String {
        let result = self
            .process_split_pdf(&params)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "split_pdf failed");
                SplitPdfResult {
                    source: Self::source_name(&params.source),
                    output_cache_key: String::new(),
                    output_page_count: 0,
                    output_path: None,
                    error: Some(e.client_message()),
                }
            });

        let response = serde_json::json!({ "results": [result] });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Merge multiple PDFs into one
    #[tool(
        description = "Merge multiple PDF files into a single PDF. PDFs are combined in the order specified. The output is always cached (output_cache_key) for chaining with other tools.

Example use cases:
- Combine multiple invoices into one document
- Merge chapters into a complete book
- Consolidate scanned pages into a single file

Source format: each element must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn merge_pdfs(&self, Parameters(params): Parameters<MergePdfsParams>) -> String {
        let result = self
            .process_merge_pdfs(&params)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "merge_pdfs failed");
                MergePdfsResult {
                    source_count: params.sources.len() as u32,
                    output_cache_key: String::new(),
                    output_page_count: 0,
                    output_path: None,
                    error: Some(e.client_message()),
                }
            });

        let response = serde_json::json!({ "results": [result] });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Add password protection to a PDF
    #[tool(
        description = "Add password protection to a PDF file using 256-bit AES encryption.

Features:
- Set user password (required to open)
- Set owner password (required to change permissions)
- Control print permission: \"full\", \"low\" (low resolution), or \"none\"
- Control copy permission (text/image extraction)
- Control modify permission (document editing)

The output is always cached (output_cache_key) for chaining with other tools.

Source format: must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn protect_pdf(&self, Parameters(params): Parameters<ProtectPdfParams>) -> String {
        let result = self
            .process_protect_pdf(&params)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "protect_pdf failed");
                ProtectPdfResult {
                    source: Self::source_name(&params.source),
                    output_cache_key: String::new(),
                    output_page_count: 0,
                    output_path: None,
                    error: Some(e.client_message()),
                }
            });

        let response = serde_json::json!({ "results": [result] });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Remove password protection from a PDF
    #[tool(
        description = "Remove password protection from an encrypted PDF. Requires the correct password.

The output is always cached (output_cache_key) for chaining with other tools.

Source format: must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn unprotect_pdf(&self, Parameters(params): Parameters<UnprotectPdfParams>) -> String {
        let result = self
            .process_unprotect_pdf(&params)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "unprotect_pdf failed");
                UnprotectPdfResult {
                    source: Self::source_name(&params.source),
                    output_cache_key: String::new(),
                    output_page_count: 0,
                    output_path: None,
                    error: Some(e.client_message()),
                }
            });

        let response = serde_json::json!({ "results": [result] });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Extract links from PDF files
    #[tool(
        description = "Extract hyperlinks from PDF files. Returns both external URLs and internal page links.

Returns for each link:
- URL (for external links)
- Destination page (for internal links)
- Link text (if extractable from the link area)
- Bounding rectangle

Source format: each element must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn extract_links(&self, Parameters(params): Parameters<ExtractLinksParams>) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_extract_links(source, &params)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "extract_links failed");
                    ExtractLinksResult {
                        source: Self::source_name(source),
                        cache_key: None,
                        links: vec![],
                        total_count: 0,
                        error: Some(e.client_message()),
                    }
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Get page information from PDF files
    #[tool(
        description = "Get detailed information about each page in a PDF. Useful for planning LLM context usage.

Returns for each page:
- Dimensions (width, height in points; 1 point = 1/72 inch)
- Rotation and orientation
- Character count and word count
- Estimated token count (model-dependent approximation)
- File size when extracted (calculated by default, use skip_file_sizes=true to disable)

Token estimation note:
Token counts are approximate and vary by model (GPT, Claude, etc.):
- Latin/English: ~4 characters per token
- CJK (Chinese/Japanese/Korean): ~2 tokens per character
Use as rough guidance for context window planning only.

Also returns totals across all pages for context planning.

Source format: each element must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn get_page_info(&self, Parameters(params): Parameters<GetPageInfoParams>) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_get_page_info(source, &params)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "get_page_info failed");
                    GetPageInfoResult {
                        source: Self::source_name(source),
                        cache_key: None,
                        pages: vec![],
                        total_pages: 0,
                        total_chars: 0,
                        total_words: 0,
                        total_estimated_token_count: 0,
                        error: Some(e.client_message()),
                    }
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Compress a PDF to reduce file size
    #[tool(
        description = "Compress a PDF file to reduce its size. Uses stream optimization, object deduplication, and image optimization.

Compression options:
- object_streams: \"generate\" (best compression), \"preserve\", or \"disable\"
- compression_level: 1-9 (higher = better compression but slower)

The output is always cached (output_cache_key) for chaining with other tools.

Source format: must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn compress_pdf(&self, Parameters(params): Parameters<CompressPdfParams>) -> String {
        let result = self
            .process_compress_pdf(&params)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "compress_pdf failed");
                CompressPdfResult {
                    source: Self::source_name(&params.source),
                    output_cache_key: String::new(),
                    original_size: 0,
                    compressed_size: 0,
                    compression_ratio: 1.0,
                    bytes_saved: 0,
                    output_page_count: 0,
                    output_path: None,
                    error: Some(e.client_message()),
                }
            });

        let response = serde_json::json!({ "results": [result] });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Convert PDF pages to images
    #[tool(
        description = "Render PDF pages as PNG images (base64-encoded). Enables Vision LLMs to understand visual layouts, charts, diagrams, and scanned content.

Options:
- width: Target width in pixels (default: 1200). Aspect ratio is preserved.
- height: Target height in pixels.
- scale: Scale factor relative to PDF page size (e.g., 2.0). Overrides width/height.

Returns base64-encoded PNG data per page, suitable for direct use with Vision models.

Source format: each element must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn convert_page_to_image(
        &self,
        Parameters(params): Parameters<ConvertPageToImageParams>,
    ) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_convert_page_to_image(source, &params)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "convert_page_to_image failed");
                    ConvertPageToImageResult {
                        source: Self::source_name(source),
                        cache_key: None,
                        pages: vec![],
                        error: Some(e.client_message()),
                    }
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Extract form fields from PDF files
    #[tool(
        description = "Extract form fields from PDF files. Returns field names, types, current values, and properties.

Supported field types:
- text: Text input fields (returns current value)
- checkbox: Checkbox fields (returns checked state)
- radio_button: Radio button fields (returns selected state)
- combo_box: Dropdown select fields (returns options and selection)
- list_box: List select fields (returns options and selection)
- push_button: Button fields
- signature: Digital signature fields

Useful for understanding PDF forms before filling them with fill_form.

Source format: each element must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn extract_form_fields(
        &self,
        Parameters(params): Parameters<ExtractFormFieldsParams>,
    ) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_extract_form_fields(source, &params)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "extract_form_fields failed");
                    ExtractFormFieldsResult {
                        source: Self::source_name(source),
                        cache_key: None,
                        fields: vec![],
                        total_fields: 0,
                        error: Some(e.client_message()),
                    }
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Fill form fields in a PDF
    #[tool(
        description = "Fill form fields in a PDF and produce a new PDF. Supports text fields, checkboxes, and radio buttons.

Each field_value entry specifies:
- name: The field name (use extract_form_fields to discover names)
- value: Text value (for text fields)
- checked: Boolean (for checkbox/radio fields)

Limitations:
- ComboBox/ListBox selection is not supported (read-only via extract_form_fields)
- Fields are matched by name; unmatched fields are reported as skipped

The output is always cached (output_cache_key) for chaining with other tools.

Source format: must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn fill_form(&self, Parameters(params): Parameters<FillFormParams>) -> String {
        let result = self
            .process_fill_form(&params)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "fill_form failed");
                FillFormResult {
                    source: Self::source_name(&params.source),
                    output_cache_key: String::new(),
                    fields_filled: 0,
                    fields_skipped: vec![],
                    output_page_count: 0,
                    output_path: None,
                    error: Some(e.client_message()),
                }
            });

        let response = serde_json::json!({ "results": [result] });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Get comprehensive PDF structure summary
    #[tool(
        description = "Get a comprehensive one-call overview of a PDF's structure. Helps decide how to process a document.

Returns:
- Basic info: page count, file size, metadata
- Content stats: total chars, words, estimated tokens per page
- Structure: outline/bookmarks presence and count
- Per-page details: dimensions, char/word counts, has_images/links/annotations flags
- Form info: field count and types
- Security: encryption status

Use this as a first step to understand a PDF before applying other tools.

Source format: each element must be one of {\"path\": \"/absolute/path.pdf\"}, {\"url\": \"https://...\"}, {\"base64\": \"...\"}, or {\"cache_key\": \"...\"}"
    )]
    async fn summarize_structure(
        &self,
        Parameters(params): Parameters<SummarizeStructureParams>,
    ) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_summarize_structure(source, &params)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "summarize_structure failed");
                    SummarizeStructureResult {
                        source: Self::source_name(source),
                        cache_key: None,
                        page_count: 0,
                        file_size: 0,
                        metadata: None,
                        has_outline: false,
                        outline_items: 0,
                        total_chars: 0,
                        total_words: 0,
                        total_estimated_tokens: 0,
                        pages: vec![],
                        total_images: 0,
                        total_links: 0,
                        total_annotations: 0,
                        has_form: false,
                        form_field_count: 0,
                        form_field_types: HashMap::new(),
                        is_encrypted: false,
                        error: Some(e.client_message()),
                    }
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// List PDF files in a directory
    #[tool(
        description = "List PDF files in a directory. Useful for discovering PDFs before processing them.

Returns for each file:
- Full path (can be used directly with other tools)
- Filename
- File size in bytes
- Last modified time

Supports recursive search and glob pattern filtering."
    )]
    async fn list_pdfs(&self, Parameters(params): Parameters<ListPdfsParams>) -> String {
        let result = self.process_list_pdfs(&params).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "list_pdfs failed");
            ListPdfsResult {
                directory: params.directory.clone(),
                files: vec![],
                total_count: 0,
                error: Some(e.client_message()),
            }
        });

        let response = serde_json::json!({ "results": [result] });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }
}

impl PdfServer {
    fn source_name(source: &PdfSource) -> String {
        match source {
            PdfSource::Path { path } => path.clone(),
            PdfSource::Base64 { .. } => "<base64>".to_string(),
            PdfSource::Url { url } => url.clone(),
            PdfSource::CacheRef { cache_key } => format!("<cache:{}>", cache_key),
        }
    }

    async fn resolve_source(&self, source: &PdfSource) -> crate::error::Result<ResolvedPdf> {
        match source {
            PdfSource::Path { path } => {
                self.validate_path_access(path)?;
                resolve_path(path)
            }
            PdfSource::Base64 { base64 } => resolve_base64(base64),
            PdfSource::Url { url } => {
                resolve_url(
                    url,
                    self.config.allow_private_urls,
                    self.config.max_download_bytes,
                )
                .await
            }
            PdfSource::CacheRef { cache_key } => resolve_cache(cache_key, &self.cache).await,
        }
    }

    /// Validate that a path is within allowed resource directories.
    /// If no resource_dirs are configured, all paths are allowed (backward compatible).
    fn validate_path_access(&self, path: &str) -> crate::error::Result<std::path::PathBuf> {
        if self.config.resource_dirs.is_empty() {
            return Ok(std::path::PathBuf::from(path));
        }

        let canonical = std::fs::canonicalize(path).map_err(|_| {
            crate::error::Error::PathAccessDenied {
                path: path.to_string(),
            }
        })?;

        for dir in &self.config.resource_dirs {
            if let Ok(canonical_dir) = std::fs::canonicalize(dir) {
                if canonical.starts_with(&canonical_dir) {
                    return Ok(canonical);
                }
            }
        }

        Err(crate::error::Error::PathAccessDenied {
            path: path.to_string(),
        })
    }

    /// Validate that an output path is within allowed resource directories.
    /// Canonicalizes the parent directory since the output file may not exist yet.
    fn validate_output_path_access(
        &self,
        path: &str,
    ) -> crate::error::Result<std::path::PathBuf> {
        if self.config.resource_dirs.is_empty() {
            return Ok(std::path::PathBuf::from(path));
        }

        let path_obj = std::path::Path::new(path);
        let parent = path_obj
            .parent()
            .unwrap_or(std::path::Path::new("."));

        let canonical_parent = std::fs::canonicalize(parent).map_err(|_| {
            crate::error::Error::PathAccessDenied {
                path: path.to_string(),
            }
        })?;

        let canonical_target = canonical_parent.join(
            path_obj
                .file_name()
                .unwrap_or(std::ffi::OsStr::new("")),
        );

        for dir in &self.config.resource_dirs {
            if let Ok(canonical_dir) = std::fs::canonicalize(dir) {
                if canonical_target.starts_with(&canonical_dir) {
                    return Ok(canonical_target);
                }
            }
        }

        Err(crate::error::Error::PathAccessDenied {
            path: path.to_string(),
        })
    }

    /// Write output data to a file path, with sandbox validation.
    fn write_output(
        &self,
        output_path: &Option<String>,
        data: &[u8],
    ) -> crate::error::Result<Option<String>> {
        if let Some(ref path_str) = output_path {
            self.validate_output_path_access(path_str)?;

            let path = Path::new(path_str);

            // Create parent directories if they don't exist
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }

            std::fs::write(path, data)?;
            Ok(Some(path_str.clone()))
        } else {
            Ok(None)
        }
    }

    async fn process_extract_text(
        &self,
        source: &PdfSource,
        params: &ExtractTextParams,
    ) -> crate::error::Result<ExtractTextResult> {
        let resolved = self.resolve_source(source).await?;
        let source_name = resolved.source_name.clone();

        // Cache if requested
        let cache_key = if params.cache {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        // Move CPU-heavy PDF work to blocking thread pool
        let data = resolved.data;
        let password = params.password.clone();
        let include_metadata = params.include_metadata;
        let include_images = params.include_images;
        let pages_param = params.pages.clone();

        let (metadata, pages, images) = tokio::task::spawn_blocking(move || {
            // Open PDF
            let reader = PdfReader::open_bytes(&data, password.as_deref())?;

            // Get metadata
            let metadata = if include_metadata {
                let meta = reader.metadata();
                Some(PdfMetadata {
                    title: meta.title.clone(),
                    author: meta.author.clone(),
                    subject: meta.subject.clone(),
                    creator: meta.creator.clone(),
                    producer: meta.producer.clone(),
                    creation_date: meta.creation_date.clone(),
                    modification_date: meta.modification_date.clone(),
                    page_count: reader.page_count(),
                })
            } else {
                None
            };

            // Determine which pages to extract
            let pages_to_extract = if let Some(ref page_range) = pages_param {
                parse_page_range(page_range, reader.page_count())?
            } else {
                (1..=reader.page_count()).collect()
            };

            // Extract text with LLM-optimized settings
            let config = TextExtractionConfig::default();
            let page_texts = extract_text_with_options(
                &data,
                password.as_deref(),
                Some(&pages_to_extract),
                &config,
            )?;
            let pages: Vec<PageContent> = page_texts
                .into_iter()
                .map(|(page, text)| PageContent { page, text })
                .collect();

            // Extract images if requested
            let images = if include_images {
                let extracted = extract_images_from_pages(
                    &data,
                    password.as_deref(),
                    &pages_to_extract,
                )?;
                Some(
                    extracted
                        .into_iter()
                        .map(|img| ImageInfo {
                            page: img.page,
                            index: img.index,
                            width: img.width,
                            height: img.height,
                            data_base64: img.data_base64,
                            mime_type: img.mime_type,
                        })
                        .collect(),
                )
            } else {
                None
            };

            Ok::<_, crate::error::Error>((metadata, pages, images))
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        Ok(ExtractTextResult {
            source: source_name,
            cache_key,
            metadata,
            pages,
            images,
            error: None,
        })
    }

    async fn process_extract_metadata(
        &self,
        source: &PdfSource,
        params: &ExtractMetadataParams,
    ) -> crate::error::Result<ExtractMetadataResult> {
        let resolved = self.resolve_source(source).await?;
        let source_name = resolved.source_name.clone();

        // Cache if requested
        let cache_key = if params.cache {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        let data = resolved.data;
        let password = params.password.clone();

        let result = tokio::task::spawn_blocking(move || {
            let reader = PdfReader::open_bytes_metadata_only(&data, password.as_deref())?;
            let meta = reader.metadata();
            Ok::<_, crate::error::Error>(ExtractMetadataResult {
                source: source_name,
                cache_key,
                title: meta.title.clone(),
                author: meta.author.clone(),
                subject: meta.subject.clone(),
                creator: meta.creator.clone(),
                producer: meta.producer.clone(),
                creation_date: meta.creation_date.clone(),
                modification_date: meta.modification_date.clone(),
                page_count: reader.page_count(),
                error: None,
            })
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        Ok(result)
    }

    async fn process_extract_outline(
        &self,
        source: &PdfSource,
        params: &ExtractOutlineParams,
    ) -> crate::error::Result<ExtractOutlineResult> {
        let resolved = self.resolve_source(source).await?;
        let source_name = resolved.source_name.clone();

        // Cache if requested
        let cache_key = if params.cache {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        let data = resolved.data;
        let password = params.password.clone();

        let outline = tokio::task::spawn_blocking(move || {
            let reader = PdfReader::open_bytes(&data, password.as_deref())?;
            let pdf_outline = reader.get_outline();
            Ok::<_, crate::error::Error>(Self::convert_outline(pdf_outline))
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        Ok(ExtractOutlineResult {
            source: source_name,
            cache_key,
            outline,
            error: None,
        })
    }

    fn convert_outline(items: Vec<crate::pdf::OutlineItem>) -> Vec<OutlineEntry> {
        items
            .into_iter()
            .map(|item| OutlineEntry {
                title: item.title,
                page: item.page,
                children: Self::convert_outline(item.children),
            })
            .collect()
    }

    async fn process_search(
        &self,
        source: &PdfSource,
        params: &SearchParams,
    ) -> crate::error::Result<SearchResult> {
        let resolved = self.resolve_source(source).await?;
        let source_name = resolved.source_name.clone();

        // Cache if requested
        let cache_key = if params.cache {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        let data = resolved.data;
        let password = params.password.clone();
        let query = params.query.clone();
        let case_sensitive = params.case_sensitive;
        let max_results = params.max_results;
        let context_chars = params.context_chars;

        let (matches, total_matches) = tokio::task::spawn_blocking(move || {
            let reader = PdfReader::open_bytes(&data, password.as_deref())?;
            let page_matches = reader.search(&query, case_sensitive);

            let mut matches = Vec::new();
            let mut total_matches: u32 = 0;

            for (page, text) in page_matches {
                let search_text = if case_sensitive {
                    text.clone()
                } else {
                    text.to_lowercase()
                };
                let q = if case_sensitive {
                    query.clone()
                } else {
                    query.to_lowercase()
                };

                let mut start = 0;
                while let Some(pos) = search_text[start..].find(&q) {
                    let actual_pos = start + pos;
                    let context_start = actual_pos.saturating_sub(context_chars as usize);
                    let context_end =
                        (actual_pos + q.len() + context_chars as usize).min(text.len());
                    let context = text[context_start..context_end].to_string();

                    matches.push(SearchMatch {
                        page,
                        context,
                        position: actual_pos,
                    });

                    total_matches += 1;
                    if total_matches >= max_results {
                        break;
                    }

                    start = actual_pos + 1;
                }

                if total_matches >= max_results {
                    break;
                }
            }

            Ok::<_, crate::error::Error>((matches, total_matches))
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        Ok(SearchResult {
            source: source_name,
            cache_key,
            matches,
            total_matches,
            error: None,
        })
    }

    async fn process_extract_annotations(
        &self,
        source: &PdfSource,
        params: &ExtractAnnotationsParams,
    ) -> crate::error::Result<ExtractAnnotationsResult> {
        let resolved = self.resolve_source(source).await?;
        let source_name = resolved.source_name.clone();

        // Cache if requested
        let cache_key = if params.cache {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        let data = resolved.data;
        let password = params.password.clone();
        let pages_param = params.pages.clone();
        let annotation_types_param = params.annotation_types.clone();

        let annotations = tokio::task::spawn_blocking(move || {
            let reader = PdfReader::open_bytes_metadata_only(&data, password.as_deref())?;
            let page_count = reader.page_count();

            let page_numbers = if let Some(ref page_range) = pages_param {
                Some(parse_page_range(page_range, page_count)?)
            } else {
                None
            };

            let annotation_types = if annotation_types_param.is_empty() {
                None
            } else {
                Some(annotation_types_param.as_slice())
            };

            let pdf_annotations = extract_annotations(
                &data,
                password.as_deref(),
                page_numbers.as_deref(),
                annotation_types,
            )?;

            let annotations: Vec<AnnotationInfo> = pdf_annotations
                .into_iter()
                .map(|ann| AnnotationInfo {
                    page: ann.page,
                    annotation_type: ann.annotation_type,
                    contents: ann.contents,
                    author: ann.author,
                    created: ann.created,
                    modified: ann.modified,
                    bounds: ann.bounds.map(|(left, top, right, bottom)| RectInfo {
                        left,
                        top,
                        right,
                        bottom,
                    }),
                    highlighted_text: ann.highlighted_text,
                    color: ann.color,
                })
                .collect();

            Ok::<_, crate::error::Error>(annotations)
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        let total_count = annotations.len() as u32;

        Ok(ExtractAnnotationsResult {
            source: source_name,
            cache_key,
            annotations,
            total_count,
            error: None,
        })
    }

    async fn process_split_pdf(
        &self,
        params: &SplitPdfParams,
    ) -> crate::error::Result<SplitPdfResult> {
        let resolved = self.resolve_source(&params.source).await?;
        let source_name = resolved.source_name.clone();

        let data = resolved.data;
        let pages = params.pages.clone();
        let password = params.password.clone();

        let (output_data, output_page_count) = tokio::task::spawn_blocking(move || {
            let output_data =
                QpdfWrapper::split_pages(&data, &pages, password.as_deref())?;
            let output_page_count =
                QpdfWrapper::get_page_count(&output_data, password.as_deref())?;
            Ok::<_, crate::error::Error>((output_data, output_page_count))
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        // Always cache the output for chaining operations
        let output_cache_key = {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), output_data.clone());
            key
        };

        // Save to file if output_path is specified
        let output_path = self.write_output(&params.output_path, &output_data)?;

        Ok(SplitPdfResult {
            source: source_name,
            output_cache_key,
            output_page_count,
            output_path,
            error: None,
        })
    }

    async fn process_merge_pdfs(
        &self,
        params: &MergePdfsParams,
    ) -> crate::error::Result<MergePdfsResult> {
        if params.sources.is_empty() {
            return Err(crate::error::Error::QpdfError {
                reason: "No PDF sources provided".to_string(),
            });
        }

        // Resolve all sources (async)
        let mut resolved_pdfs: Vec<Vec<u8>> = Vec::new();
        for source in &params.sources {
            let resolved = self.resolve_source(source).await?;
            resolved_pdfs.push(resolved.data);
        }

        let (output_data, output_page_count) = tokio::task::spawn_blocking(move || {
            let pdf_refs: Vec<&[u8]> = resolved_pdfs.iter().map(|v| v.as_slice()).collect();
            let output_data = QpdfWrapper::merge(&pdf_refs)?;
            let output_page_count = QpdfWrapper::get_page_count(&output_data, None)?;
            Ok::<_, crate::error::Error>((output_data, output_page_count))
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        // Always cache the output for chaining operations
        let output_cache_key = {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), output_data.clone());
            key
        };

        // Save to file if output_path is specified
        let output_path = self.write_output(&params.output_path, &output_data)?;

        Ok(MergePdfsResult {
            source_count: params.sources.len() as u32,
            output_cache_key,
            output_page_count,
            output_path,
            error: None,
        })
    }

    async fn process_protect_pdf(
        &self,
        params: &ProtectPdfParams,
    ) -> crate::error::Result<ProtectPdfResult> {
        let resolved = self.resolve_source(&params.source).await?;
        let source_name = resolved.source_name.clone();

        let data = resolved.data;
        let user_password = params.user_password.clone();
        let owner_password = params.owner_password.clone();
        let allow_print = params.allow_print.clone();
        let allow_copy = params.allow_copy;
        let allow_modify = params.allow_modify;
        let password = params.password.clone();

        let (output_data, output_page_count) = tokio::task::spawn_blocking(move || {
            let output_data = QpdfWrapper::encrypt(
                &data,
                &user_password,
                owner_password.as_deref(),
                &allow_print,
                allow_copy,
                allow_modify,
                password.as_deref(),
            )?;
            let output_page_count =
                QpdfWrapper::get_page_count(&output_data, Some(&user_password))?;
            Ok::<_, crate::error::Error>((output_data, output_page_count))
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        // Always cache the output for chaining operations
        let output_cache_key = {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), output_data.clone());
            key
        };

        // Save to file if output_path is specified
        let output_path = self.write_output(&params.output_path, &output_data)?;

        Ok(ProtectPdfResult {
            source: source_name,
            output_cache_key,
            output_page_count,
            output_path,
            error: None,
        })
    }

    async fn process_unprotect_pdf(
        &self,
        params: &UnprotectPdfParams,
    ) -> crate::error::Result<UnprotectPdfResult> {
        let resolved = self.resolve_source(&params.source).await?;
        let source_name = resolved.source_name.clone();

        let data = resolved.data;
        let password = params.password.clone();

        let (output_data, output_page_count) = tokio::task::spawn_blocking(move || {
            let output_data = QpdfWrapper::decrypt(&data, &password)?;
            let output_page_count = QpdfWrapper::get_page_count(&output_data, None)?;
            Ok::<_, crate::error::Error>((output_data, output_page_count))
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        // Always cache the output for chaining operations
        let output_cache_key = {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), output_data.clone());
            key
        };

        // Save to file if output_path is specified
        let output_path = self.write_output(&params.output_path, &output_data)?;

        Ok(UnprotectPdfResult {
            source: source_name,
            output_cache_key,
            output_page_count,
            output_path,
            error: None,
        })
    }

    async fn process_extract_links(
        &self,
        source: &PdfSource,
        params: &ExtractLinksParams,
    ) -> crate::error::Result<ExtractLinksResult> {
        let resolved = self.resolve_source(source).await?;
        let source_name = resolved.source_name.clone();

        // Cache if requested
        let cache_key = if params.cache {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        let data = resolved.data;
        let password = params.password.clone();
        let pages_param = params.pages.clone();

        let links = tokio::task::spawn_blocking(move || {
            let page_numbers = if let Some(ref page_range) = pages_param {
                let reader =
                    PdfReader::open_bytes_metadata_only(&data, password.as_deref())?;
                let page_count = reader.page_count();
                Some(parse_page_range(page_range, page_count)?)
            } else {
                None
            };

            let pdf_links = extract_links(
                &data,
                password.as_deref(),
                page_numbers.as_deref(),
            )?;

            let links: Vec<LinkInfo> = pdf_links
                .into_iter()
                .map(|link| LinkInfo {
                    page: link.page,
                    url: link.url,
                    dest_page: link.dest_page,
                    bounds: link.bounds.map(|(left, top, right, bottom)| RectInfo {
                        left,
                        top,
                        right,
                        bottom,
                    }),
                    text: link.text,
                })
                .collect();

            Ok::<_, crate::error::Error>(links)
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        let total_count = links.len() as u32;

        Ok(ExtractLinksResult {
            source: source_name,
            cache_key,
            links,
            total_count,
            error: None,
        })
    }

    async fn process_get_page_info(
        &self,
        source: &PdfSource,
        params: &GetPageInfoParams,
    ) -> crate::error::Result<GetPageInfoResult> {
        let resolved = self.resolve_source(source).await?;
        let source_name = resolved.source_name.clone();

        // Cache if requested
        let cache_key = if params.cache {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        let data = resolved.data;
        let password = params.password.clone();
        let skip_file_sizes = params.skip_file_sizes;

        let result = tokio::task::spawn_blocking(move || {
            let page_infos = get_page_info(&data, password.as_deref())?;
            let total_pages = page_infos.len() as u32;

            let file_sizes: Option<Vec<usize>> = if !skip_file_sizes {
                let mut sizes = Vec::with_capacity(total_pages as usize);
                for page_num in 1..=total_pages {
                    let page_data = QpdfWrapper::split_pages(
                        &data,
                        &page_num.to_string(),
                        password.as_deref(),
                    )?;
                    sizes.push(page_data.len());
                }
                Some(sizes)
            } else {
                None
            };

            let mut total_chars = 0usize;
            let mut total_words = 0usize;
            let mut total_estimated_token_count = 0usize;

            let pages: Vec<PageInfo> = page_infos
                .into_iter()
                .enumerate()
                .map(|(idx, info)| {
                    total_chars += info.char_count;
                    total_words += info.word_count;
                    total_estimated_token_count += info.estimated_token_count;
                    PageInfo {
                        page: info.page,
                        width: info.width,
                        height: info.height,
                        rotation: info.rotation,
                        orientation: info.orientation,
                        char_count: info.char_count,
                        word_count: info.word_count,
                        estimated_token_count: info.estimated_token_count,
                        file_size: file_sizes.as_ref().map(|s| s[idx]),
                    }
                })
                .collect();

            Ok::<_, crate::error::Error>((pages, total_pages, total_chars, total_words, total_estimated_token_count))
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        let (pages, total_pages, total_chars, total_words, total_estimated_token_count) = result;

        Ok(GetPageInfoResult {
            source: source_name,
            cache_key,
            pages,
            total_pages,
            total_chars,
            total_words,
            total_estimated_token_count,
            error: None,
        })
    }

    async fn process_compress_pdf(
        &self,
        params: &CompressPdfParams,
    ) -> crate::error::Result<CompressPdfResult> {
        let resolved = self.resolve_source(&params.source).await?;
        let source_name = resolved.source_name.clone();
        let original_size = resolved.data.len();

        let data = resolved.data;
        let password = params.password.clone();
        let object_streams = params.object_streams.clone();
        let compression_level = params.compression_level;

        let (output_data, output_page_count) = tokio::task::spawn_blocking(move || {
            let output_data = QpdfWrapper::compress(
                &data,
                password.as_deref(),
                Some(&object_streams),
                Some(compression_level),
            )?;
            let output_page_count = QpdfWrapper::get_page_count(&output_data, None)?;
            Ok::<_, crate::error::Error>((output_data, output_page_count))
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        let compressed_size = output_data.len();
        let compression_ratio = if original_size > 0 {
            compressed_size as f32 / original_size as f32
        } else {
            1.0
        };
        let bytes_saved = original_size as i64 - compressed_size as i64;

        // Always cache the output for chaining operations
        let output_cache_key = {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), output_data.clone());
            key
        };

        // Save to file if output_path is specified
        let output_path = self.write_output(&params.output_path, &output_data)?;

        Ok(CompressPdfResult {
            source: source_name,
            output_cache_key,
            original_size,
            compressed_size,
            compression_ratio,
            bytes_saved,
            output_page_count,
            output_path,
            error: None,
        })
    }

    pub async fn process_convert_page_to_image(
        &self,
        source: &PdfSource,
        params: &ConvertPageToImageParams,
    ) -> crate::error::Result<ConvertPageToImageResult> {
        // Validate image rendering limits
        if let Some(scale) = params.scale {
            if scale <= 0.0 || scale > self.config.max_image_scale {
                return Err(crate::error::Error::ImageDimensionExceeded {
                    detail: format!(
                        "scale must be between 0.0 (exclusive) and {} (inclusive), got {}",
                        self.config.max_image_scale, scale
                    ),
                });
            }
        }
        if let (Some(w), Some(h)) = (params.width, params.height) {
            let pixel_area = w as u64 * h as u64;
            if pixel_area > self.config.max_image_pixels {
                return Err(crate::error::Error::ImageDimensionExceeded {
                    detail: format!(
                        "pixel area {}x{} = {} exceeds maximum {} pixels",
                        w, h, pixel_area, self.config.max_image_pixels
                    ),
                });
            }
        }

        let resolved = self.resolve_source(source).await?;
        let source_name = resolved.source_name.clone();

        // Cache if requested
        let cache_key = if params.cache {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        let data = resolved.data;
        let password = params.password.clone();
        let pages_param = params.pages.clone();
        let width = params.width;
        let height = params.height;
        let scale = params.scale;

        let pages = tokio::task::spawn_blocking(move || {
            let reader =
                PdfReader::open_bytes_metadata_only(&data, password.as_deref())?;
            let page_count = reader.page_count();

            let page_numbers = if let Some(ref page_range) = pages_param {
                parse_page_range(page_range, page_count)?
            } else {
                (1..=page_count).collect()
            };

            let rendered = render_pages_to_images(
                &data,
                password.as_deref(),
                &page_numbers,
                width,
                height,
                scale,
            )?;

            let pages: Vec<RenderedPageInfo> = rendered
                .into_iter()
                .map(|rp| RenderedPageInfo {
                    page: rp.page,
                    width: rp.width,
                    height: rp.height,
                    data_base64: rp.data_base64,
                    mime_type: rp.mime_type,
                })
                .collect();

            Ok::<_, crate::error::Error>(pages)
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        Ok(ConvertPageToImageResult {
            source: source_name,
            cache_key,
            pages,
            error: None,
        })
    }

    pub async fn process_extract_form_fields(
        &self,
        source: &PdfSource,
        params: &ExtractFormFieldsParams,
    ) -> crate::error::Result<ExtractFormFieldsResult> {
        let resolved = self.resolve_source(source).await?;
        let source_name = resolved.source_name.clone();

        // Cache if requested
        let cache_key = if params.cache {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        let data = resolved.data;
        let password = params.password.clone();
        let pages_param = params.pages.clone();

        let fields = tokio::task::spawn_blocking(move || {
            let page_numbers = if let Some(ref page_range) = pages_param {
                let reader =
                    PdfReader::open_bytes_metadata_only(&data, password.as_deref())?;
                let page_count = reader.page_count();
                Some(parse_page_range(page_range, page_count)?)
            } else {
                None
            };

            let pdf_fields = extract_form_fields(
                &data,
                password.as_deref(),
                page_numbers.as_deref(),
            )?;

            let fields: Vec<FormFieldInfoResponse> = pdf_fields
                .into_iter()
                .map(|f| FormFieldInfoResponse {
                    page: f.page,
                    name: f.name,
                    field_type: f.field_type,
                    value: f.value,
                    is_checked: f.is_checked,
                    is_read_only: f.is_read_only,
                    is_required: f.is_required,
                    options: f.options.map(|opts| {
                        opts.into_iter()
                            .map(|o| FormFieldOptionInfoResponse {
                                label: o.label,
                                is_selected: o.is_selected,
                            })
                            .collect()
                    }),
                    properties: FormFieldPropertiesResponse {
                        is_multiline: f.properties.is_multiline,
                        is_password: f.properties.is_password,
                        is_editable: f.properties.is_editable,
                        is_multiselect: f.properties.is_multiselect,
                    },
                })
                .collect();

            Ok::<_, crate::error::Error>(fields)
        })
        .await
        .map_err(|e| crate::error::Error::Pdfium {
            reason: format!("Task join error: {}", e),
        })??;

        let total_fields = fields.len();

        Ok(ExtractFormFieldsResult {
            source: source_name,
            cache_key,
            fields,
            total_fields,
            error: None,
        })
    }

    pub async fn process_fill_form(
        &self,
        params: &FillFormParams,
    ) -> crate::error::Result<FillFormResult> {
        let resolved = self.resolve_source(&params.source).await?;
        let source_name = resolved.source_name.clone();

        // Convert params to reader types
        let field_values: Vec<crate::pdf::FormFieldValue> = params
            .field_values
            .iter()
            .map(|fv| crate::pdf::FormFieldValue {
                name: fv.name.clone(),
                value: fv.value.clone(),
                checked: fv.checked,
            })
            .collect();

        let data = resolved.data;
        let password = params.password.clone();

        // Fill form fields and get page count — CPU-bound PDFium work
        let (output_data, fill_result, output_page_count) =
            tokio::task::spawn_blocking(move || {
                let (output_data, fill_result) =
                    fill_form_fields(&data, password.as_deref(), &field_values)?;
                let output_page_count = PdfReader::open_bytes_metadata_only(&output_data, None)
                    .map(|r| r.page_count())
                    .unwrap_or(0);
                Ok::<_, crate::error::Error>((output_data, fill_result, output_page_count))
            })
            .await
            .map_err(|e| crate::error::Error::Pdfium {
                reason: format!("Task join error: {}", e),
            })??;

        // Always cache the output for chaining operations
        let output_cache_key = {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), output_data.clone());
            key
        };

        // Save to file if output_path is specified
        let output_path = self.write_output(&params.output_path, &output_data)?;

        let fields_skipped = fill_result
            .fields_skipped
            .into_iter()
            .map(|s| SkippedFieldInfo {
                name: s.name,
                reason: s.reason,
            })
            .collect();

        Ok(FillFormResult {
            source: source_name,
            output_cache_key,
            fields_filled: fill_result.fields_filled,
            fields_skipped,
            output_page_count,
            output_path,
            error: None,
        })
    }

    pub async fn process_summarize_structure(
        &self,
        source: &PdfSource,
        params: &SummarizeStructureParams,
    ) -> crate::error::Result<SummarizeStructureResult> {
        let resolved = self.resolve_source(source).await?;
        let source_name = resolved.source_name.clone();
        let file_size = resolved.data.len();

        // Cache if requested
        let cache_key = if params.cache {
            let cache_guard = self.cache.write().await;
            let key = cache_guard.generate_unique_key();
            cache_guard.put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        // Check if encrypted (try opening without password first)
        let is_encrypted = params.password.is_some();

        let data = resolved.data;
        let password = params.password.clone();

        // All PDFium/qpdf CPU-bound work in spawn_blocking
        let (page_count, metadata, has_outline, outline_items, page_summaries,
             total_chars, total_words, total_estimated_tokens, total_images,
             total_links, total_annotations, total_form_fields, form_field_types) =
            tokio::task::spawn_blocking(move || {
                // Open PDF with PdfReader to get metadata and outline, then drop to free PDFium
                let (page_count, metadata, has_outline, outline_items) = {
                    let reader = PdfReader::open_bytes(&data, password.as_deref())?;
                    let page_count = reader.page_count();
                    let meta = reader.metadata();
                    let metadata = Some(PdfMetadata {
                        title: meta.title.clone(),
                        author: meta.author.clone(),
                        subject: meta.subject.clone(),
                        creator: meta.creator.clone(),
                        producer: meta.producer.clone(),
                        creation_date: meta.creation_date.clone(),
                        modification_date: meta.modification_date.clone(),
                        page_count,
                    });
                    let outline = reader.get_outline();
                    let has_outline = !outline.is_empty();
                    let outline_items = Self::count_outline_items(&outline);
                    (page_count, metadata, has_outline, outline_items)
                }; // reader dropped here, PDFium library freed

                // Get page info — includes dimensions, content stats, image/annotation/link/form counts
                // All gathered in a single PDFium load to avoid SIGSEGV from multiple library loads
                let page_infos = get_page_info(&data, password.as_deref())?;

                // Build per-page summaries and aggregate totals
                let mut total_chars = 0usize;
                let mut total_words = 0usize;
                let mut total_estimated_tokens = 0usize;
                let mut total_images = 0u32;
                let mut total_links = 0u32;
                let mut total_annotations = 0u32;
                let mut total_form_fields = 0u32;
                let mut form_field_types: HashMap<String, u32> = HashMap::new();

                let mut page_summaries = Vec::with_capacity(page_count as usize);

                for info in &page_infos {
                    total_chars += info.char_count;
                    total_words += info.word_count;
                    total_estimated_tokens += info.estimated_token_count;
                    total_images += info.image_count as u32;
                    total_links += info.link_count as u32;
                    total_annotations += info.annotation_count as u32;
                    total_form_fields += info.form_field_count as u32;

                    for ft in &info.form_field_types {
                        *form_field_types.entry(ft.clone()).or_insert(0) += 1;
                    }

                    page_summaries.push(PageSummary {
                        page: info.page,
                        width: info.width,
                        height: info.height,
                        char_count: info.char_count,
                        word_count: info.word_count,
                        has_images: info.image_count > 0,
                        has_links: info.link_count > 0,
                        has_annotations: info.annotation_count > 0,
                    });
                }

                Ok::<_, crate::error::Error>((
                    page_count, metadata, has_outline, outline_items, page_summaries,
                    total_chars, total_words, total_estimated_tokens, total_images,
                    total_links, total_annotations, total_form_fields, form_field_types,
                ))
            })
            .await
            .map_err(|e| crate::error::Error::Pdfium {
                reason: format!("Task join error: {}", e),
            })??;

        let has_form = total_form_fields > 0;
        let form_field_count = total_form_fields;

        Ok(SummarizeStructureResult {
            source: source_name,
            cache_key,
            page_count,
            file_size,
            metadata,
            has_outline,
            outline_items,
            total_chars,
            total_words,
            total_estimated_tokens,
            pages: page_summaries,
            total_images,
            total_links,
            total_annotations,
            has_form,
            form_field_count,
            form_field_types,
            is_encrypted,
            error: None,
        })
    }

    fn count_outline_items(items: &[crate::pdf::OutlineItem]) -> u32 {
        let mut count = items.len() as u32;
        for item in items {
            count += Self::count_outline_items(&item.children);
        }
        count
    }

    /// List PDF files in a directory (public for testing)
    pub fn process_list_pdfs_public(
        &self,
        params: &ListPdfsParams,
    ) -> crate::error::Result<ListPdfsResult> {
        self.process_list_pdfs(params)
    }

    fn process_list_pdfs(&self, params: &ListPdfsParams) -> crate::error::Result<ListPdfsResult> {
        // Sandbox check: if resource_dirs are configured, directory must be within them
        if !self.config.resource_dirs.is_empty() {
            let canonical = std::fs::canonicalize(&params.directory).map_err(|_| {
                crate::error::Error::PathAccessDenied {
                    path: params.directory.clone(),
                }
            })?;
            let allowed = self.config.resource_dirs.iter().any(|dir| {
                std::fs::canonicalize(dir)
                    .map(|cd| canonical.starts_with(&cd))
                    .unwrap_or(false)
            });
            if !allowed {
                return Err(crate::error::Error::PathAccessDenied {
                    path: params.directory.clone(),
                });
            }
        }

        let dir_path = Path::new(&params.directory);

        if !dir_path.exists() {
            return Err(crate::error::Error::PdfNotFound {
                path: params.directory.clone(),
            });
        }

        if !dir_path.is_dir() {
            return Err(crate::error::Error::InvalidPdf {
                reason: format!("{} is not a directory", params.directory),
            });
        }

        let mut files = Vec::new();

        // Compile glob pattern if provided
        let pattern = params
            .pattern
            .as_ref()
            .and_then(|p| glob::Pattern::new(p).ok());

        Self::collect_pdfs(dir_path, params.recursive, &pattern, &mut files)?;

        // Sort by path for consistent ordering
        files.sort_by(|a, b| a.path.cmp(&b.path));

        let total_count = files.len() as u32;

        Ok(ListPdfsResult {
            directory: params.directory.clone(),
            files,
            total_count,
            error: None,
        })
    }

    fn collect_pdfs(
        dir: &Path,
        recursive: bool,
        pattern: &Option<glob::Pattern>,
        files: &mut Vec<PdfFileInfo>,
    ) -> crate::error::Result<()> {
        let entries = std::fs::read_dir(dir).map_err(crate::error::Error::Io)?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue, // Skip entries we can't read
            };

            let path = entry.path();

            if path.is_dir() {
                if recursive {
                    // Recursively collect from subdirectory
                    let _ = Self::collect_pdfs(&path, recursive, pattern, files);
                }
            } else if path.is_file() {
                // Check if it's a PDF file
                if let Some(ext) = path.extension() {
                    if ext.eq_ignore_ascii_case("pdf") {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();

                        // Apply pattern filter if provided
                        if let Some(ref pat) = pattern {
                            if !pat.matches(&name) {
                                continue;
                            }
                        }

                        // Get file metadata
                        let metadata = std::fs::metadata(&path).ok();
                        let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                        let modified = metadata
                            .as_ref()
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| {
                                chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                                    .map(|dt| dt.to_rfc3339())
                                    .unwrap_or_default()
                            });

                        files.push(PdfFileInfo {
                            path: path.to_string_lossy().to_string(),
                            name,
                            size,
                            modified,
                        });
                    }
                }
            }
        }

        Ok(())
    }
}

impl Default for PdfServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_handler]
impl ServerHandler for PdfServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "PDF MCP Server provides tools for extracting text, outlines, and searching PDFs. \
                 PDF files in configured directories are also exposed as resources."
                    .into(),
            ),
        }
    }

    /// List available PDF resources from configured directories
    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let mut resources = Vec::new();

        for dir in self.config.resource_dirs.iter() {
            let params = ListPdfsParams {
                directory: dir.clone(),
                recursive: true,
                pattern: None,
            };

            if let Ok(list_result) = self.process_list_pdfs_public(&params) {
                for file in list_result.files {
                    let uri = format!("file://{}", file.path);
                    let mut resource = RawResource::new(uri.clone(), file.name.clone());
                    resource.mime_type = Some("application/pdf".to_string());
                    resource.description = Some(format!(
                        "PDF file ({} bytes){}",
                        file.size,
                        file.modified
                            .as_ref()
                            .map(|m| format!(", modified: {}", m))
                            .unwrap_or_default()
                    ));
                    resource.size = Some(file.size as u32);

                    resources.push(Annotated {
                        raw: resource,
                        annotations: None,
                    });
                }
            }
        }

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
            meta: Default::default(),
        })
    }

    /// Read a PDF resource and return its text content
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        let uri = &request.uri;

        // Parse file:// URI to get the path
        let path = if uri.starts_with("file://") {
            uri.strip_prefix("file://").unwrap_or(uri)
        } else {
            return Err(ErrorData::invalid_params(
                "Only file:// URIs are supported",
                None,
            ));
        };

        // Check if the path is within a configured resource directory (using canonicalize to prevent traversal)
        let is_allowed = if self.config.resource_dirs.is_empty() {
            true
        } else if let Ok(canonical_path) = std::fs::canonicalize(path) {
            self.config.resource_dirs.iter().any(|dir| {
                std::fs::canonicalize(dir)
                    .map(|cd| canonical_path.starts_with(&cd))
                    .unwrap_or(false)
            })
        } else {
            false
        };

        if !is_allowed {
            return Err(ErrorData::invalid_params(
                "Resource not found in configured directories",
                None,
            ));
        }

        // Extract text from the PDF
        let source = PdfSource::Path {
            path: path.to_string(),
        };

        match self
            .process_extract_text(
                &source,
                &ExtractTextParams {
                    sources: vec![source.clone()],
                    pages: None,
                    include_metadata: true,
                    include_images: false,
                    password: None,
                    cache: false,
                },
            )
            .await
        {
            Ok(result) => {
                // Combine all page content
                let text = result
                    .pages
                    .iter()
                    .map(|p| format!("--- Page {} ---\n{}", p.page, p.text))
                    .collect::<Vec<_>>()
                    .join("\n\n");

                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::TextResourceContents {
                        uri: uri.clone(),
                        mime_type: Some("text/plain".to_string()),
                        text,
                        meta: Default::default(),
                    }],
                })
            }
            Err(e) => {
                tracing::warn!(error = %e, "read_resource failed");
                Err(ErrorData::internal_error(e.client_message(), None))
            }
        }
    }
}

/// Run the MCP server without resource directories
pub async fn run_server() -> Result<()> {
    run_server_with_config(ServerConfig::default()).await
}

/// Run the MCP server with specified resource directories
pub async fn run_server_with_dirs(resource_dirs: Vec<String>) -> Result<()> {
    run_server_with_config(ServerConfig {
        resource_dirs,
        ..ServerConfig::default()
    })
    .await
}

/// Run the MCP server with full configuration
pub async fn run_server_with_config(config: ServerConfig) -> Result<()> {
    let server = PdfServer::with_config(config);

    tracing::info!("PDF MCP Server ready, waiting for connections...");

    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("tests/fixtures");
        path.push(name);
        path
    }

    #[test]
    fn test_source_name() {
        assert_eq!(
            PdfServer::source_name(&PdfSource::Path {
                path: "/test.pdf".to_string()
            }),
            "/test.pdf"
        );
        assert_eq!(
            PdfServer::source_name(&PdfSource::Base64 {
                base64: "...".to_string()
            }),
            "<base64>"
        );
        assert_eq!(
            PdfServer::source_name(&PdfSource::Url {
                url: "https://example.com/test.pdf".to_string()
            }),
            "https://example.com/test.pdf"
        );
        assert_eq!(
            PdfServer::source_name(&PdfSource::CacheRef {
                cache_key: "abc123".to_string()
            }),
            "<cache:abc123>"
        );
    }

    #[test]
    fn test_params_deserialization() {
        let json = r#"{
            "sources": [{"path": "/test.pdf"}],
            "pages": "1-5"
        }"#;
        let params: ExtractTextParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.pages, Some("1-5".to_string()));
        assert!(params.include_metadata);
    }

    #[test]
    fn test_pdf_source_deserialization() {
        // Path source
        let json = r#"{"path": "/test.pdf"}"#;
        let source: PdfSource = serde_json::from_str(json).unwrap();
        assert!(matches!(source, PdfSource::Path { .. }));

        // Base64 source
        let json = r#"{"base64": "JVBERi0xLjQ="}"#;
        let source: PdfSource = serde_json::from_str(json).unwrap();
        assert!(matches!(source, PdfSource::Base64 { .. }));

        // URL source
        let json = r#"{"url": "https://example.com/test.pdf"}"#;
        let source: PdfSource = serde_json::from_str(json).unwrap();
        assert!(matches!(source, PdfSource::Url { .. }));

        // Cache reference
        let json = r#"{"cache_key": "abc123"}"#;
        let source: PdfSource = serde_json::from_str(json).unwrap();
        assert!(matches!(source, PdfSource::CacheRef { .. }));
    }

    #[test]
    fn test_default_values() {
        assert!(default_true());
        assert_eq!(default_max_results(), 100);
        assert_eq!(default_context_chars(), 50);
        assert_eq!(default_full(), "full");
        assert_eq!(default_compression_level(), 9);
        assert_eq!(default_object_streams(), "generate");
    }

    #[tokio::test]
    async fn test_process_extract_text() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
        };
        let params = ExtractTextParams {
            sources: vec![source.clone()],
            pages: None,
            include_metadata: true,
            include_images: false,
            password: None,
            cache: false,
        };

        let result = server.process_extract_text(&source, &params).await.unwrap();
        assert!(result.error.is_none());
        assert!(!result.pages.is_empty());
        assert!(result.metadata.is_some());
    }

    #[tokio::test]
    async fn test_process_extract_text_with_cache() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
        };
        let params = ExtractTextParams {
            sources: vec![source.clone()],
            pages: None,
            include_metadata: true,
            include_images: false,
            password: None,
            cache: true,
        };

        let result = server.process_extract_text(&source, &params).await.unwrap();
        assert!(result.cache_key.is_some());

        // Verify cache entry exists
        let cache_key = result.cache_key.unwrap();
        let cache = server.cache.read().await;
        assert!(cache.get(&cache_key).is_some());
    }

    #[tokio::test]
    async fn test_process_extract_text_with_pages() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("tracemonkey.pdf")
                .to_string_lossy()
                .to_string(),
        };
        let params = ExtractTextParams {
            sources: vec![source.clone()],
            pages: Some("1-2".to_string()),
            include_metadata: false,
            include_images: false,
            password: None,
            cache: false,
        };

        let result = server.process_extract_text(&source, &params).await.unwrap();
        assert!(result.error.is_none());
        assert_eq!(result.pages.len(), 2);
    }

    #[tokio::test]
    async fn test_process_extract_metadata() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
        };
        let params = ExtractMetadataParams {
            sources: vec![source.clone()],
            password: None,
            cache: false,
        };

        let result = server
            .process_extract_metadata(&source, &params)
            .await
            .unwrap();
        assert!(result.error.is_none());
        assert!(result.page_count > 0);
    }

    #[tokio::test]
    async fn test_process_extract_outline() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("test-with-outline-and-images.pdf")
                .to_string_lossy()
                .to_string(),
        };
        let params = ExtractOutlineParams {
            sources: vec![source.clone()],
            password: None,
            cache: false,
        };

        let result = server
            .process_extract_outline(&source, &params)
            .await
            .unwrap();
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_process_search() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("tracemonkey.pdf")
                .to_string_lossy()
                .to_string(),
        };
        let params = SearchParams {
            sources: vec![source.clone()],
            query: "trace".to_string(),
            case_sensitive: false,
            max_results: 10,
            context_chars: 20,
            password: None,
            cache: false,
        };

        let result = server.process_search(&source, &params).await.unwrap();
        assert!(result.error.is_none());
        assert!(result.total_matches > 0);
    }

    #[tokio::test]
    async fn test_process_search_case_sensitive() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("tracemonkey.pdf")
                .to_string_lossy()
                .to_string(),
        };
        let params = SearchParams {
            sources: vec![source.clone()],
            query: "Trace".to_string(),
            case_sensitive: true,
            max_results: 10,
            context_chars: 20,
            password: None,
            cache: false,
        };

        let result = server.process_search(&source, &params).await.unwrap();
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_process_extract_annotations() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
        };
        let params = ExtractAnnotationsParams {
            sources: vec![source.clone()],
            annotation_types: vec![],
            pages: None,
            password: None,
            cache: false,
        };

        let result = server
            .process_extract_annotations(&source, &params)
            .await
            .unwrap();
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_process_split_pdf() {
        let server = PdfServer::new();
        let params = SplitPdfParams {
            source: PdfSource::Path {
                path: fixture_path("tracemonkey.pdf")
                    .to_string_lossy()
                    .to_string(),
            },
            pages: "1-2".to_string(),
            output_path: None,
            password: None,
        };

        let result = server.process_split_pdf(&params).await.unwrap();
        assert!(result.error.is_none());
        assert_eq!(result.output_page_count, 2);
        assert!(!result.output_cache_key.is_empty());
    }

    #[tokio::test]
    async fn test_process_merge_pdfs() {
        let server = PdfServer::new();
        let params = MergePdfsParams {
            sources: vec![
                PdfSource::Path {
                    path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
                },
                PdfSource::Path {
                    path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
                },
            ],
            output_path: None,
        };

        let result = server.process_merge_pdfs(&params).await.unwrap();
        assert!(result.error.is_none());
        assert_eq!(result.source_count, 2);
        assert!(!result.output_cache_key.is_empty());
    }

    #[tokio::test]
    async fn test_process_merge_pdfs_empty() {
        let server = PdfServer::new();
        let params = MergePdfsParams {
            sources: vec![],
            output_path: None,
        };

        let result = server.process_merge_pdfs(&params).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_process_protect_pdf() {
        let server = PdfServer::new();
        let params = ProtectPdfParams {
            source: PdfSource::Path {
                path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
            },
            user_password: "test123".to_string(),
            owner_password: None,
            allow_print: "full".to_string(),
            allow_copy: true,
            allow_modify: true,
            output_path: None,
            password: None,
        };

        let result = server.process_protect_pdf(&params).await.unwrap();
        assert!(result.error.is_none());
        assert!(!result.output_cache_key.is_empty());
    }

    #[tokio::test]
    async fn test_process_unprotect_pdf() {
        let server = PdfServer::new();
        let params = UnprotectPdfParams {
            source: PdfSource::Path {
                path: fixture_path("password-protected.pdf")
                    .to_string_lossy()
                    .to_string(),
            },
            password: "testpass".to_string(),
            output_path: None,
        };

        let result = server.process_unprotect_pdf(&params).await.unwrap();
        assert!(result.error.is_none());
        assert!(!result.output_cache_key.is_empty());
    }

    #[tokio::test]
    async fn test_process_extract_links() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
        };
        let params = ExtractLinksParams {
            sources: vec![source.clone()],
            pages: None,
            password: None,
            cache: false,
        };

        let result = server
            .process_extract_links(&source, &params)
            .await
            .unwrap();
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_process_get_page_info() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
        };
        let params = GetPageInfoParams {
            sources: vec![source.clone()],
            password: None,
            cache: false,
            skip_file_sizes: true,
        };

        let result = server
            .process_get_page_info(&source, &params)
            .await
            .unwrap();
        assert!(result.error.is_none());
        assert!(result.total_pages > 0);
    }

    #[tokio::test]
    async fn test_process_get_page_info_with_file_sizes() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
        };
        let params = GetPageInfoParams {
            sources: vec![source.clone()],
            password: None,
            cache: false,
            skip_file_sizes: false,
        };

        let result = server
            .process_get_page_info(&source, &params)
            .await
            .unwrap();
        assert!(result.error.is_none());
        // File sizes should be calculated
        for page in &result.pages {
            assert!(page.file_size.is_some());
        }
    }

    #[tokio::test]
    async fn test_process_compress_pdf() {
        let server = PdfServer::new();
        let params = CompressPdfParams {
            source: PdfSource::Path {
                path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
            },
            object_streams: "generate".to_string(),
            compression_level: 9,
            output_path: None,
            password: None,
        };

        let result = server.process_compress_pdf(&params).await.unwrap();
        assert!(result.error.is_none());
        assert!(!result.output_cache_key.is_empty());
        assert!(result.original_size > 0);
    }

    #[tokio::test]
    async fn test_resolve_source_path() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
        };

        let resolved = server.resolve_source(&source).await.unwrap();
        assert!(!resolved.data.is_empty());
    }

    #[tokio::test]
    async fn test_resolve_source_base64() {
        let server = PdfServer::new();
        // Read a PDF file and encode it as base64
        let pdf_path = fixture_path("dummy.pdf");
        let pdf_data = std::fs::read(&pdf_path).unwrap();
        let base64_data =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &pdf_data);

        let source = PdfSource::Base64 {
            base64: base64_data,
        };

        let resolved = server.resolve_source(&source).await.unwrap();
        assert_eq!(resolved.data, pdf_data);
    }

    #[tokio::test]
    async fn test_resolve_source_cache() {
        let server = PdfServer::new();

        // First, add something to cache
        let pdf_path = fixture_path("dummy.pdf");
        let pdf_data = std::fs::read(&pdf_path).unwrap();
        let cache_key = CacheManager::generate_key();
        server
            .cache
            .write()
            .await
            .put(cache_key.clone(), pdf_data.clone());

        // Now resolve from cache
        let source = PdfSource::CacheRef {
            cache_key: cache_key.clone(),
        };

        let resolved = server.resolve_source(&source).await.unwrap();
        assert_eq!(resolved.data, pdf_data);
    }

    #[tokio::test]
    async fn test_resolve_source_cache_not_found() {
        let server = PdfServer::new();
        let source = PdfSource::CacheRef {
            cache_key: "nonexistent_key".to_string(),
        };

        let result = server.resolve_source(&source).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_password_protected_operations() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("password-protected.pdf")
                .to_string_lossy()
                .to_string(),
        };

        // Test extract_text with password
        let params = ExtractTextParams {
            sources: vec![source.clone()],
            pages: None,
            include_metadata: true,
            include_images: false,
            password: Some("testpass".to_string()),
            cache: false,
        };

        let result = server.process_extract_text(&source, &params).await.unwrap();
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_convert_outline() {
        use crate::pdf::OutlineItem;

        let items = vec![OutlineItem {
            title: "Chapter 1".to_string(),
            page: Some(1),
            children: vec![OutlineItem {
                title: "Section 1.1".to_string(),
                page: Some(2),
                children: vec![],
            }],
        }];

        let converted = PdfServer::convert_outline(items);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].title, "Chapter 1");
        assert_eq!(converted[0].children.len(), 1);
        assert_eq!(converted[0].children[0].title, "Section 1.1");
    }

    #[test]
    fn test_pdf_server_default() {
        let server = PdfServer::default();
        // Just verify it doesn't panic
        let _ = server;
    }

    // ========================================================================
    // Path sandboxing tests
    // ========================================================================

    #[test]
    fn test_validate_path_no_resource_dirs_allows_all() {
        let server = PdfServer::new();
        // No resource_dirs → all paths allowed
        let result = server.validate_path_access(
            &fixture_path("dummy.pdf").to_string_lossy(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_within_resource_dir() {
        let fixtures_dir = {
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("tests/fixtures");
            p
        };
        let server = PdfServer::with_config(ServerConfig {
            resource_dirs: vec![fixtures_dir.to_string_lossy().to_string()],
            ..ServerConfig::default()
        });
        let result = server.validate_path_access(
            &fixture_path("dummy.pdf").to_string_lossy(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_outside_resource_dir_denied() {
        let fixtures_dir = {
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("tests/fixtures");
            p
        };
        let server = PdfServer::with_config(ServerConfig {
            resource_dirs: vec![fixtures_dir.to_string_lossy().to_string()],
            ..ServerConfig::default()
        });
        // Cargo.toml is outside tests/fixtures
        let cargo_toml = {
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("Cargo.toml");
            p
        };
        let result = server.validate_path_access(&cargo_toml.to_string_lossy());
        assert!(matches!(
            result,
            Err(crate::error::Error::PathAccessDenied { .. })
        ));
    }

    #[test]
    fn test_validate_path_traversal_denied() {
        let fixtures_dir = {
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("tests/fixtures");
            p
        };
        let server = PdfServer::with_config(ServerConfig {
            resource_dirs: vec![fixtures_dir.to_string_lossy().to_string()],
            ..ServerConfig::default()
        });
        // Attempt path traversal
        let traversal = format!(
            "{}/../../Cargo.toml",
            fixtures_dir.to_string_lossy()
        );
        let result = server.validate_path_access(&traversal);
        assert!(matches!(
            result,
            Err(crate::error::Error::PathAccessDenied { .. })
        ));
    }

    #[test]
    fn test_validate_output_path_within_resource_dir() {
        let fixtures_dir = {
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("tests/fixtures");
            p
        };
        let server = PdfServer::with_config(ServerConfig {
            resource_dirs: vec![fixtures_dir.to_string_lossy().to_string()],
            ..ServerConfig::default()
        });
        let output = format!("{}/output.pdf", fixtures_dir.to_string_lossy());
        let result = server.validate_output_path_access(&output);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_output_path_outside_denied() {
        let fixtures_dir = {
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("tests/fixtures");
            p
        };
        let server = PdfServer::with_config(ServerConfig {
            resource_dirs: vec![fixtures_dir.to_string_lossy().to_string()],
            ..ServerConfig::default()
        });
        let result = server.validate_output_path_access("/tmp/evil.pdf");
        assert!(matches!(
            result,
            Err(crate::error::Error::PathAccessDenied { .. })
        ));
    }

    // ========================================================================
    // Image rendering limit tests
    // ========================================================================

    #[tokio::test]
    async fn test_image_scale_too_large() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
        };
        let params = ConvertPageToImageParams {
            sources: vec![source.clone()],
            pages: Some("1".to_string()),
            width: None,
            height: None,
            scale: Some(999.0),
            password: None,
            cache: false,
        };
        let result = server.process_convert_page_to_image(&source, &params).await;
        assert!(matches!(
            result,
            Err(crate::error::Error::ImageDimensionExceeded { .. })
        ));
    }

    #[tokio::test]
    async fn test_image_scale_zero() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
        };
        let params = ConvertPageToImageParams {
            sources: vec![source.clone()],
            pages: Some("1".to_string()),
            width: None,
            height: None,
            scale: Some(0.0),
            password: None,
            cache: false,
        };
        let result = server.process_convert_page_to_image(&source, &params).await;
        assert!(matches!(
            result,
            Err(crate::error::Error::ImageDimensionExceeded { .. })
        ));
    }

    #[tokio::test]
    async fn test_image_pixel_area_too_large() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
        };
        let params = ConvertPageToImageParams {
            sources: vec![source.clone()],
            pages: Some("1".to_string()),
            width: Some(50000),
            height: Some(50000),
            scale: None,
            password: None,
            cache: false,
        };
        let result = server.process_convert_page_to_image(&source, &params).await;
        assert!(matches!(
            result,
            Err(crate::error::Error::ImageDimensionExceeded { .. })
        ));
    }

    #[tokio::test]
    async fn test_image_valid_scale() {
        let server = PdfServer::new();
        let source = PdfSource::Path {
            path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
        };
        let params = ConvertPageToImageParams {
            sources: vec![source.clone()],
            pages: Some("1".to_string()),
            width: None,
            height: None,
            scale: Some(1.0),
            password: None,
            cache: false,
        };
        let result = server.process_convert_page_to_image(&source, &params).await;
        assert!(result.is_ok());
    }

    // ========================================================================
    // list_pdfs sandboxing tests
    // ========================================================================

    #[test]
    fn test_list_pdfs_sandbox_allowed() {
        let fixtures_dir = {
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("tests/fixtures");
            p
        };
        let server = PdfServer::with_config(ServerConfig {
            resource_dirs: vec![fixtures_dir.to_string_lossy().to_string()],
            ..ServerConfig::default()
        });
        let params = ListPdfsParams {
            directory: fixtures_dir.to_string_lossy().to_string(),
            recursive: false,
            pattern: None,
        };
        let result = server.process_list_pdfs(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_pdfs_sandbox_denied() {
        let fixtures_dir = {
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("tests/fixtures");
            p
        };
        let server = PdfServer::with_config(ServerConfig {
            resource_dirs: vec![fixtures_dir.to_string_lossy().to_string()],
            ..ServerConfig::default()
        });
        let params = ListPdfsParams {
            directory: "/tmp".to_string(),
            recursive: false,
            pattern: None,
        };
        let result = server.process_list_pdfs(&params);
        assert!(matches!(
            result,
            Err(crate::error::Error::PathAccessDenied { .. })
        ));
    }

    // ========================================================================
    // ServerConfig tests
    // ========================================================================

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert!(config.resource_dirs.is_empty());
        assert!(!config.allow_private_urls);
        assert_eq!(config.max_download_bytes, 100 * 1024 * 1024);
        assert_eq!(config.cache_max_bytes, 512 * 1024 * 1024);
        assert_eq!(config.cache_max_entries, 100);
        assert_eq!(config.max_image_scale, 10.0);
        assert_eq!(config.max_image_pixels, 100_000_000);
    }

    #[tokio::test]
    async fn test_resolve_source_path_sandboxed_denied() {
        let fixtures_dir = {
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("tests/fixtures");
            p
        };
        let server = PdfServer::with_config(ServerConfig {
            resource_dirs: vec![fixtures_dir.to_string_lossy().to_string()],
            ..ServerConfig::default()
        });
        // Try to access a file outside the sandbox
        let cargo_toml = {
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("Cargo.toml");
            p
        };
        let source = PdfSource::Path {
            path: cargo_toml.to_string_lossy().to_string(),
        };
        let result = server.resolve_source(&source).await;
        assert!(matches!(
            result,
            Err(crate::error::Error::PathAccessDenied { .. })
        ));
    }
}
