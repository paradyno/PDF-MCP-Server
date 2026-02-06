//! MCP Server implementation using rmcp

use crate::pdf::{
    extract_annotations, extract_images_from_pages, extract_links, get_page_info, parse_page_range,
    PdfReader, QpdfWrapper,
};
use crate::source::{
    resolve_base64, resolve_cache, resolve_path, resolve_url, CacheManager, ResolvedPdf,
};
use anyhow::Result;
use rmcp::{
    handler::server::tool::ToolRouter, handler::server::wrapper::Parameters, model::*,
    schemars::JsonSchema, tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// PDF source specification
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
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

/// PDF MCP Server
#[derive(Clone)]
pub struct PdfServer {
    cache: Arc<RwLock<CacheManager>>,
    tool_router: ToolRouter<Self>,
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
// Tool implementations
// ============================================================================

#[tool_router]
impl PdfServer {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(CacheManager::new(100))),
            tool_router: Self::tool_router(),
        }
    }

    /// Extract text content from PDF files
    #[tool(
        description = "Extract text content from PDF files. Supports page selection, metadata extraction, and batch processing of multiple PDFs."
    )]
    async fn extract_text(&self, Parameters(params): Parameters<ExtractTextParams>) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_extract_text(source, &params)
                .await
                .unwrap_or_else(|e| ExtractTextResult {
                    source: Self::source_name(source),
                    cache_key: None,
                    metadata: None,
                    pages: vec![],
                    images: None,
                    error: Some(e.to_string()),
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Extract PDF bookmarks/table of contents
    #[tool(
        description = "Extract PDF bookmarks/table of contents with page numbers and hierarchy."
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
                .unwrap_or_else(|e| ExtractOutlineResult {
                    source: Self::source_name(source),
                    cache_key: None,
                    outline: vec![],
                    error: Some(e.to_string()),
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Search for text within PDF files
    #[tool(
        description = "Search for text within PDF files. Returns matching text with context and page locations."
    )]
    async fn search(&self, Parameters(params): Parameters<SearchParams>) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_search(source, &params)
                .await
                .unwrap_or_else(|e| SearchResult {
                    source: Self::source_name(source),
                    cache_key: None,
                    matches: vec![],
                    total_matches: 0,
                    error: Some(e.to_string()),
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Extract PDF metadata without loading full content
    #[tool(
        description = "Extract PDF metadata (author, title, creation date, page count, etc.) without loading full content. Fast operation for getting document information."
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
                .unwrap_or_else(|e| ExtractMetadataResult {
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
                    error: Some(e.to_string()),
                });
            results.push(result);
        }

        let response = serde_json::json!({ "results": results });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Extract annotations from PDF files
    #[tool(
        description = "Extract annotations (highlights, comments, underlines, etc.) from PDF files. Returns annotation content, author, dates, and highlighted text."
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
                .unwrap_or_else(|e| ExtractAnnotationsResult {
                    source: Self::source_name(source),
                    cache_key: None,
                    annotations: vec![],
                    total_count: 0,
                    error: Some(e.to_string()),
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
- Duplicate: \"1,1,1\" (page 1 three times)"
    )]
    async fn split_pdf(&self, Parameters(params): Parameters<SplitPdfParams>) -> String {
        let result = self
            .process_split_pdf(&params)
            .await
            .unwrap_or_else(|e| SplitPdfResult {
                source: Self::source_name(&params.source),
                output_cache_key: String::new(),
                output_page_count: 0,
                output_path: None,
                error: Some(e.to_string()),
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
- Consolidate scanned pages into a single file"
    )]
    async fn merge_pdfs(&self, Parameters(params): Parameters<MergePdfsParams>) -> String {
        let result = self
            .process_merge_pdfs(&params)
            .await
            .unwrap_or_else(|e| MergePdfsResult {
                source_count: params.sources.len() as u32,
                output_cache_key: String::new(),
                output_page_count: 0,
                output_path: None,
                error: Some(e.to_string()),
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

The output is always cached (output_cache_key) for chaining with other tools."
    )]
    async fn protect_pdf(&self, Parameters(params): Parameters<ProtectPdfParams>) -> String {
        let result = self
            .process_protect_pdf(&params)
            .await
            .unwrap_or_else(|e| ProtectPdfResult {
                source: Self::source_name(&params.source),
                output_cache_key: String::new(),
                output_page_count: 0,
                output_path: None,
                error: Some(e.to_string()),
            });

        let response = serde_json::json!({ "results": [result] });
        serde_json::to_string_pretty(&response).unwrap_or_default()
    }

    /// Remove password protection from a PDF
    #[tool(
        description = "Remove password protection from an encrypted PDF. Requires the correct password.

The output is always cached (output_cache_key) for chaining with other tools."
    )]
    async fn unprotect_pdf(&self, Parameters(params): Parameters<UnprotectPdfParams>) -> String {
        let result = self
            .process_unprotect_pdf(&params)
            .await
            .unwrap_or_else(|e| UnprotectPdfResult {
                source: Self::source_name(&params.source),
                output_cache_key: String::new(),
                output_page_count: 0,
                output_path: None,
                error: Some(e.to_string()),
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
- Bounding rectangle"
    )]
    async fn extract_links(&self, Parameters(params): Parameters<ExtractLinksParams>) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_extract_links(source, &params)
                .await
                .unwrap_or_else(|e| ExtractLinksResult {
                    source: Self::source_name(source),
                    cache_key: None,
                    links: vec![],
                    total_count: 0,
                    error: Some(e.to_string()),
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

Also returns totals across all pages for context planning."
    )]
    async fn get_page_info(&self, Parameters(params): Parameters<GetPageInfoParams>) -> String {
        let mut results = Vec::new();

        for source in &params.sources {
            let result = self
                .process_get_page_info(source, &params)
                .await
                .unwrap_or_else(|e| GetPageInfoResult {
                    source: Self::source_name(source),
                    cache_key: None,
                    pages: vec![],
                    total_pages: 0,
                    total_chars: 0,
                    total_words: 0,
                    total_estimated_token_count: 0,
                    error: Some(e.to_string()),
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

The output is always cached (output_cache_key) for chaining with other tools."
    )]
    async fn compress_pdf(&self, Parameters(params): Parameters<CompressPdfParams>) -> String {
        let result = self
            .process_compress_pdf(&params)
            .await
            .unwrap_or_else(|e| CompressPdfResult {
                source: Self::source_name(&params.source),
                output_cache_key: String::new(),
                original_size: 0,
                compressed_size: 0,
                compression_ratio: 1.0,
                bytes_saved: 0,
                output_page_count: 0,
                output_path: None,
                error: Some(e.to_string()),
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
            PdfSource::Path { path } => resolve_path(path),
            PdfSource::Base64 { base64 } => resolve_base64(base64),
            PdfSource::Url { url } => resolve_url(url).await,
            PdfSource::CacheRef { cache_key } => resolve_cache(cache_key, &self.cache).await,
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
            let key = CacheManager::generate_key();
            self.cache
                .write()
                .await
                .put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        // Open PDF
        let reader = PdfReader::open_bytes(&resolved.data, params.password.as_deref())?;

        // Get metadata
        let metadata = if params.include_metadata {
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
        let pages_to_extract = if let Some(ref page_range) = params.pages {
            parse_page_range(page_range, reader.page_count())?
        } else {
            (1..=reader.page_count()).collect()
        };

        // Extract text
        let page_texts = reader.extract_pages_text(&pages_to_extract)?;
        let pages: Vec<PageContent> = page_texts
            .into_iter()
            .map(|(page, text)| PageContent { page, text })
            .collect();

        // Extract images if requested
        let images = if params.include_images {
            let extracted = extract_images_from_pages(
                &resolved.data,
                params.password.as_deref(),
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
            let key = CacheManager::generate_key();
            self.cache
                .write()
                .await
                .put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        // Open PDF and extract only metadata (no text extraction)
        let reader =
            PdfReader::open_bytes_metadata_only(&resolved.data, params.password.as_deref())?;
        let meta = reader.metadata();

        Ok(ExtractMetadataResult {
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
            let key = CacheManager::generate_key();
            self.cache
                .write()
                .await
                .put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        // Open PDF
        let reader = PdfReader::open_bytes(&resolved.data, params.password.as_deref())?;

        // Get outline
        let pdf_outline = reader.get_outline();
        let outline = Self::convert_outline(pdf_outline);

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
            let key = CacheManager::generate_key();
            self.cache
                .write()
                .await
                .put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        // Open PDF
        let reader = PdfReader::open_bytes(&resolved.data, params.password.as_deref())?;

        // Search
        let page_matches = reader.search(&params.query, params.case_sensitive);

        let mut matches = Vec::new();
        let mut total_matches: u32 = 0;

        for (page, text) in page_matches {
            let search_text = if params.case_sensitive {
                text.clone()
            } else {
                text.to_lowercase()
            };
            let query = if params.case_sensitive {
                params.query.clone()
            } else {
                params.query.to_lowercase()
            };

            // Find all occurrences
            let mut start = 0;
            while let Some(pos) = search_text[start..].find(&query) {
                let actual_pos = start + pos;

                // Extract context
                let context_start = actual_pos.saturating_sub(params.context_chars as usize);
                let context_end =
                    (actual_pos + query.len() + params.context_chars as usize).min(text.len());
                let context = text[context_start..context_end].to_string();

                matches.push(SearchMatch {
                    page,
                    context,
                    position: actual_pos,
                });

                total_matches += 1;
                if total_matches >= params.max_results {
                    break;
                }

                start = actual_pos + 1;
            }

            if total_matches >= params.max_results {
                break;
            }
        }

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
            let key = CacheManager::generate_key();
            self.cache
                .write()
                .await
                .put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        // Parse page range if specified
        // We need to get page count first, so we'll open the PDF briefly
        let reader =
            PdfReader::open_bytes_metadata_only(&resolved.data, params.password.as_deref())?;
        let page_count = reader.page_count();

        let page_numbers = if let Some(ref page_range) = params.pages {
            Some(parse_page_range(page_range, page_count)?)
        } else {
            None
        };

        // Extract annotations
        let annotation_types = if params.annotation_types.is_empty() {
            None
        } else {
            Some(params.annotation_types.as_slice())
        };

        let pdf_annotations = extract_annotations(
            &resolved.data,
            params.password.as_deref(),
            page_numbers.as_deref(),
            annotation_types,
        )?;

        // Convert to response type
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

        // Use qpdf to split pages
        let output_data =
            QpdfWrapper::split_pages(&resolved.data, &params.pages, params.password.as_deref())?;

        // Get page count of output PDF
        let output_page_count =
            QpdfWrapper::get_page_count(&output_data, params.password.as_deref())?;

        // Always cache the output for chaining operations
        let output_cache_key = CacheManager::generate_key();
        self.cache
            .write()
            .await
            .put(output_cache_key.clone(), output_data.clone());

        // Save to file if output_path is specified
        let output_path = if let Some(ref path_str) = params.output_path {
            let path = Path::new(path_str);

            // Create parent directories if they don't exist
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }

            std::fs::write(path, &output_data)?;
            Some(path_str.clone())
        } else {
            None
        };

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

        // Resolve all sources
        let mut resolved_pdfs: Vec<Vec<u8>> = Vec::new();
        for source in &params.sources {
            let resolved = self.resolve_source(source).await?;
            resolved_pdfs.push(resolved.data);
        }

        // Create references for qpdf
        let pdf_refs: Vec<&[u8]> = resolved_pdfs.iter().map(|v| v.as_slice()).collect();

        // Use qpdf to merge PDFs
        let output_data = QpdfWrapper::merge(&pdf_refs)?;

        // Get page count of output PDF
        let output_page_count = QpdfWrapper::get_page_count(&output_data, None)?;

        // Always cache the output for chaining operations
        let output_cache_key = CacheManager::generate_key();
        self.cache
            .write()
            .await
            .put(output_cache_key.clone(), output_data.clone());

        // Save to file if output_path is specified
        let output_path = if let Some(ref path_str) = params.output_path {
            let path = Path::new(path_str);

            // Create parent directories if they don't exist
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }

            std::fs::write(path, &output_data)?;
            Some(path_str.clone())
        } else {
            None
        };

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

        // Use qpdf to encrypt the PDF
        let output_data = QpdfWrapper::encrypt(
            &resolved.data,
            &params.user_password,
            params.owner_password.as_deref(),
            &params.allow_print,
            params.allow_copy,
            params.allow_modify,
            params.password.as_deref(),
        )?;

        // Get page count of output PDF (need user password to open)
        let output_page_count =
            QpdfWrapper::get_page_count(&output_data, Some(&params.user_password))?;

        // Always cache the output for chaining operations
        let output_cache_key = CacheManager::generate_key();
        self.cache
            .write()
            .await
            .put(output_cache_key.clone(), output_data.clone());

        // Save to file if output_path is specified
        let output_path = if let Some(ref path_str) = params.output_path {
            let path = Path::new(path_str);

            // Create parent directories if they don't exist
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }

            std::fs::write(path, &output_data)?;
            Some(path_str.clone())
        } else {
            None
        };

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

        // Use qpdf to decrypt the PDF
        let output_data = QpdfWrapper::decrypt(&resolved.data, &params.password)?;

        // Get page count of output PDF (no password needed now)
        let output_page_count = QpdfWrapper::get_page_count(&output_data, None)?;

        // Always cache the output for chaining operations
        let output_cache_key = CacheManager::generate_key();
        self.cache
            .write()
            .await
            .put(output_cache_key.clone(), output_data.clone());

        // Save to file if output_path is specified
        let output_path = if let Some(ref path_str) = params.output_path {
            let path = Path::new(path_str);

            // Create parent directories if they don't exist
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }

            std::fs::write(path, &output_data)?;
            Some(path_str.clone())
        } else {
            None
        };

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
            let key = CacheManager::generate_key();
            self.cache
                .write()
                .await
                .put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        // Parse page range if specified
        let page_numbers = if let Some(ref page_range) = params.pages {
            let reader =
                PdfReader::open_bytes_metadata_only(&resolved.data, params.password.as_deref())?;
            let page_count = reader.page_count();
            Some(parse_page_range(page_range, page_count)?)
        } else {
            None
        };

        // Extract links
        let pdf_links = extract_links(
            &resolved.data,
            params.password.as_deref(),
            page_numbers.as_deref(),
        )?;

        // Convert to response type
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
            let key = CacheManager::generate_key();
            self.cache
                .write()
                .await
                .put(key.clone(), resolved.data.clone());
            Some(key)
        } else {
            None
        };

        // Get page info
        let page_infos = get_page_info(&resolved.data, params.password.as_deref())?;
        let total_pages = page_infos.len() as u32;

        // Calculate file sizes by default (unless skip_file_sizes is true)
        let file_sizes: Option<Vec<usize>> = if !params.skip_file_sizes {
            let mut sizes = Vec::with_capacity(total_pages as usize);
            for page_num in 1..=total_pages {
                let page_data = QpdfWrapper::split_pages(
                    &resolved.data,
                    &page_num.to_string(),
                    params.password.as_deref(),
                )?;
                sizes.push(page_data.len());
            }
            Some(sizes)
        } else {
            None
        };

        // Convert and calculate totals
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

        // Use qpdf to compress the PDF
        let output_data = QpdfWrapper::compress(
            &resolved.data,
            params.password.as_deref(),
            Some(&params.object_streams),
            Some(params.compression_level),
        )?;

        let compressed_size = output_data.len();
        let compression_ratio = if original_size > 0 {
            compressed_size as f32 / original_size as f32
        } else {
            1.0
        };
        let bytes_saved = original_size as i64 - compressed_size as i64;

        // Get page count of output PDF
        let output_page_count = QpdfWrapper::get_page_count(&output_data, None)?;

        // Always cache the output for chaining operations
        let output_cache_key = CacheManager::generate_key();
        self.cache
            .write()
            .await
            .put(output_cache_key.clone(), output_data.clone());

        // Save to file if output_path is specified
        let output_path = if let Some(ref path_str) = params.output_path {
            let path = Path::new(path_str);

            // Create parent directories if they don't exist
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }

            std::fs::write(path, &output_data)?;
            Some(path_str.clone())
        } else {
            None
        };

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
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "PDF MCP Server provides tools for extracting text, outlines, and searching PDFs."
                    .into(),
            ),
        }
    }
}

/// Run the MCP server
pub async fn run_server() -> Result<()> {
    let server = PdfServer::new();

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
}
