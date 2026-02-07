# CLAUDE.md - PDF MCP Server Development Guide

## Rules

- After making any changes to the project (code, configuration, build system, commands, etc.), always check whether `README.md` and `CLAUDE.md` need to be updated to reflect those changes. If updates are needed, propose the changes to the user.

## Project Overview

This is a Model Context Protocol (MCP) server for PDF operations, implemented in Rust. It provides tools for extracting text, metadata, annotations, images, links, and form fields from PDFs, as well as manipulation operations like splitting, merging, compression, password protection, form filling, page rendering, and structure summarization.

## Architecture

```
src/
├── main.rs           # Entry point
├── lib.rs            # Library exports
├── error.rs          # Error types (thiserror)
├── server.rs         # MCP server implementation (rmcp framework)
│                     # - Tool definitions (Params/Result structs)
│                     # - Tool handlers (#[tool] macro)
│                     # - Processing logic (process_* methods)
├── pdf/
│   ├── mod.rs        # PDF module exports
│   ├── reader.rs     # PDFium-based PDF reading (text, metadata, outline)
│   ├── annotations.rs # Annotation extraction
│   ├── images.rs     # Image extraction
│   └── qpdf.rs       # qpdf FFI wrapper via vendored qpdf crate (split, merge, encrypt, decrypt)
└── source/
    ├── mod.rs        # Source module exports
    ├── resolver.rs   # PdfSource resolution (path, base64, url, cache)
    └── cache.rs      # In-memory PDF cache (LRU)
```

## Key Technologies

- **rmcp**: Rust MCP framework for tool definitions and MCP Resources
- **pdfium-render**: PDFium bindings for PDF reading (dynamic linking)
- **qpdf** (crate): Vendored FFI bindings for PDF manipulation (no runtime dependency)
- **tokio**: Async runtime
- **serde/schemars**: JSON serialization and schema generation

## MCP Resources

The server supports MCP Resources to expose PDFs from configured directories.

### Configuration

```bash
# Command line
pdf-mcp-server --resource-dir /documents --resource-dir /data/pdfs

# Environment variable (path-separated: colon on Unix, semicolon on Windows)
PDF_RESOURCE_DIRS=/documents:/data/pdfs pdf-mcp-server
```

### Implementation

Resources are implemented via `ServerHandler` trait methods in `src/server.rs`:
- `list_resources`: Lists all PDFs in configured `resource_dirs`
- `read_resource`: Extracts text content from a PDF by `file://` URI

Resource directories are stored in `PdfServer::config.resource_dirs` and configured via:
- `PdfServer::with_config(config)` - programmatic (full config)
- `PdfServer::with_resource_dirs(dirs)` - convenience wrapper
- `run_server_with_config(config)` - server startup function (full config)
- `run_server_with_dirs(dirs)` - convenience wrapper

## Security Configuration

### ServerConfig

All security/resource settings are centralized in `ServerConfig` (`src/server.rs`):

```rust
pub struct ServerConfig {
    pub resource_dirs: Vec<String>,     // Directories for sandboxing (empty = no sandboxing)
    pub allow_private_urls: bool,       // default: false (SSRF protection on)
    pub max_download_bytes: u64,        // default: 100MB
    pub cache_max_bytes: usize,         // default: 512MB
    pub cache_max_entries: usize,       // default: 100
    pub max_image_scale: f32,           // default: 10.0
    pub max_image_pixels: u64,          // default: 100_000_000
}
```

### Path Sandboxing

When `resource_dirs` is non-empty, all file operations are sandboxed:
- `validate_path_access()` — canonicalizes path, checks `starts_with()` against each resource dir
- `validate_output_path_access()` — same but canonicalizes parent dir (file may not exist)
- `write_output()` — shared helper for all output_path blocks (replaces 6 inline copies)
- `process_list_pdfs()` — verifies directory is within resource dirs
- `read_resource()` — uses `fs::canonicalize()` to prevent traversal

### SSRF Protection & URL Downloads

In `src/source/resolver.rs`:
- `is_private_ip()` — checks against loopback, private, link-local, CGNAT, IPv6 equivalents
- `check_ssrf()` — resolves URL DNS, rejects if any IP is private
- `resolve_url(url, allow_private_urls, max_download_bytes)` — checks SSRF and enforces download limit
- Streaming download: uses `bytes_stream()` with incremental size checking to prevent OOM from infinite streams

### Cache Memory Budget

In `src/source/cache.rs`:
- `CacheManager::new(capacity, max_bytes)` — entry count + byte budget
- `put()` rejects entries > max_bytes, evicts LRU entries until byte budget satisfied
- `total_bytes()` getter for monitoring

### Image Rendering Limits

In `process_convert_page_to_image()`:
- Scale validated: `0.0 < scale <= max_image_scale`
- Pixel area validated: `width * height <= max_image_pixels` (when both specified)

### CLI Flags

| Flag | Env Var | Default |
|------|---------|---------|
| `--resource-dir <PATH>` | `PDF_RESOURCE_DIRS` | (none) |
| `--allow-private-urls` | `PDF_ALLOW_PRIVATE_URLS=1` | false |
| `--max-download-size <N>` | `PDF_MAX_DOWNLOAD_BYTES` | 104857600 (100MB) |
| `--cache-max-bytes <N>` | `PDF_CACHE_MAX_BYTES` | 536870912 (512MB) |
| `--cache-max-entries <N>` | `PDF_CACHE_MAX_ENTRIES` | 100 |
| `--max-image-scale <N>` | `PDF_MAX_IMAGE_SCALE` | 10.0 |
| `--max-image-pixels <N>` | `PDF_MAX_IMAGE_PIXELS` | 100000000 |
| — | `PDFIUM_PATH` | (system library) |

**PDFium library search order**: `PDFIUM_PATH` env var → `/opt/pdfium/lib` → system library. CWD is intentionally excluded to prevent binary planting.

## Development Commands

All commands run via Docker Compose with profiles (`dev` for development, `prod` for production):

```bash
# Build
docker compose --profile dev run --rm dev cargo build

# Run clippy (linting)
docker compose --profile dev run --rm clippy

# Format check
docker compose --profile dev run --rm dev cargo fmt --all -- --check

# Format fix
docker compose --profile dev run --rm dev cargo fmt --all

# Run tests (using nextest)
docker compose --profile dev run --rm test

# Run specific test
docker compose --profile dev run --rm dev cargo test --test integration_test test_name -- --nocapture

# Run performance benchmarks
docker compose --profile dev run --rm bench

# Build production image (minimal runtime, ~120MB)
docker compose --profile prod build production

# Clean up dev images when no longer needed
docker compose --profile dev down --rmi local
```

## Adding a New Tool

### 1. Define Params and Result types in `src/server.rs`

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MyToolParams {
    pub source: PdfSource,
    #[serde(default)]
    pub optional_field: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MyToolResult {
    pub source: String,
    pub output_cache_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
```

### 2. Add tool handler in `#[tool_router] impl PdfServer`

```rust
#[tool(description = "Tool description here")]
async fn my_tool(&self, Parameters(params): Parameters<MyToolParams>) -> String {
    let result = self
        .process_my_tool(&params)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "my_tool failed");
            MyToolResult {
                source: Self::source_name(&params.source),
                output_cache_key: String::new(),
                error: Some(e.client_message()),
            }
        });

    let response = serde_json::json!({ "results": [result] });
    serde_json::to_string_pretty(&response).unwrap_or_default()
}
```

### 3. Add processing logic in `impl PdfServer`

```rust
async fn process_my_tool(
    &self,
    params: &MyToolParams,
) -> crate::error::Result<MyToolResult> {
    let resolved = self.resolve_source(&params.source).await?;
    let source_name = resolved.source_name.clone();

    // Do processing...

    // Cache output if applicable
    let output_cache_key = {
        let cache_guard = self.cache.write().await;
        let key = cache_guard.generate_unique_key();
        cache_guard.put(key.clone(), output_data);
        key
    };

    Ok(MyToolResult {
        source: source_name,
        output_cache_key,
        error: None,
    })
}
```

### 4. Add tests in `tests/integration_test.rs`

## Working with qpdf (FFI)

The project uses the `qpdf` Rust crate with `vendored` feature, which statically links the qpdf C++ library. No external `qpdf` binary is required at runtime.

### Key APIs

All operations go through `QpdfWrapper` in `src/pdf/qpdf.rs`:

```rust
// Read PDF from memory
let qpdf = QPdf::read_from_memory(data)?;
let qpdf = QPdf::read_from_memory_encrypted(data, "password")?;

// Create empty PDF (for merge/split targets)
let dest = QPdf::empty();

// Copy pages between documents
let page = source.get_page(0).unwrap();
let copied = dest.copy_from_foreign(&page);
dest.add_page(&copied, false)?;

// Write to memory
let bytes = qpdf.writer().write_to_memory()?;

// Encryption (AES-256)
let mut writer = qpdf.writer();
writer.encryption_params(EncryptionParams::R6(EncryptionParamsR6 { ... }));

// Decryption
writer.preserve_encryption(false);
```

### Error Handling

The qpdf crate returns `qpdf::QPdfError` with an `error_code()` method. The `map_qpdf_error()` function maps these to our `Error` enum, specifically converting `QPdfErrorCode::InvalidPassword` to `Error::IncorrectPassword`.

### Page Range Parser

`parse_qpdf_page_range()` in `src/pdf/qpdf.rs` handles the page range syntax:

- `1-5`: Pages 1-5
- `1,3,5`: Specific pages
- `z`: Last page
- `r1`: Last page, `r2`: Second to last
- `z-1`: All pages reversed
- `1-z:odd`: Odd pages only
- `1-z:even`: Even pages only

## PdfSource Resolution

The server accepts multiple source types:

```rust
pub enum PdfSource {
    Path { path: String },           // File path
    Base64 { base64: String },       // Base64 encoded
    Url { url: String },             // HTTP URL
    CacheRef { cache_key: String },  // Previous operation result
}
```

## Error Handling

Define errors in `src/error.rs`:

```rust
#[derive(Error, Debug)]
pub enum Error {
    #[error("qpdf error: {reason}")]
    QpdfError { reason: String },
    #[error("Path access denied: {path}")]
    PathAccessDenied { path: String },
    #[error("SSRF blocked: {url}")]
    SsrfBlocked { url: String },
    #[error("Download too large: {size} bytes (max: {max_size} bytes)")]
    DownloadTooLarge { size: u64, max_size: u64 },
    #[error("Image dimension exceeded: {detail}")]
    ImageDimensionExceeded { detail: String },
    // ...
}
```

### Error Sanitization

`Error::client_message()` returns sanitized messages safe for MCP clients (no internal paths, library details, or file sizes). Tool handlers use:
```rust
.unwrap_or_else(|e| {
    tracing::warn!(error = %e, "tool_name failed");
    ToolResult { error: Some(e.client_message()), .. }
})
```

## Testing Patterns

### Test Fixtures

Place test PDFs in `tests/fixtures/`. Access via:

```rust
fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    path.push(name);
    path
}
```

### Available Test Fixtures

- `dummy.pdf` - Simple test PDF
- `tracemonkey.pdf` - Multi-page document
- `basicapi.pdf` - Another test document
- `test-with-outline-and-images.pdf` - PDF with bookmarks and images
- `password-protected.pdf` - Encrypted PDF (password: `testpass`)
- `form-test.pdf` - PDF with form fields (text, checkbox, choice)

## Common Patterns

### Always Cache Output for Chaining

```rust
let output_cache_key = {
    let cache_guard = self.cache.write().await;
    let key = cache_guard.generate_unique_key();
    cache_guard.put(key.clone(), output_data.clone());
    key
};
```

### Optional File Output

Use the shared `write_output()` helper which handles sandboxing validation:

```rust
let output_path = self.write_output(&params.output_path, &output_data)?;
```

### Default Values for Optional Parameters

```rust
fn default_true() -> bool { true }
fn default_full() -> String { "full".to_string() }

#[derive(Deserialize)]
struct Params {
    #[serde(default = "default_true")]
    pub include_metadata: bool,
}
```

## Gotchas

1. **qpdf FFI is sync**: All qpdf operations block. All `process_*` methods wrap sync PDF work in `tokio::task::spawn_blocking` to avoid starving async worker threads.
2. **Page indices**: qpdf crate uses 0-indexed pages, but our API uses 1-indexed. `parse_qpdf_page_range()` handles the conversion.
3. **Password errors**: qpdf crate returns `QPdfErrorCode::InvalidPassword` — mapped to `Error::IncorrectPassword` via `map_qpdf_error()`.
4. **Page counts**: Use `QpdfWrapper::get_page_count()` for encrypted PDFs after operations.
5. **Clippy**: Run before committing - the project uses strict linting.
6. **Format**: Always run `cargo fmt` before committing.
7. **PDFium multiple loads**: Calling `create_pdfium()` many times in sequence (3+) on the same PDF can cause SIGSEGV. The `summarize_structure` tool avoids this by gathering all per-page data (images, annotations, links, form fields) in a single `get_page_info()` call rather than calling separate extraction functions. When adding new aggregation tools, prefer using `PdfPageInfo` fields over separate PDFium loads.
9. **CacheManager constructor**: `CacheManager::new(capacity, max_bytes)` takes two parameters — entry count limit and byte budget. Both are configured via `ServerConfig`.
10. **resolve_url signature**: `resolve_url(url, allow_private_urls, max_download_bytes)` — SSRF check and download limits are passed from `ServerConfig`.
11. **process_list_pdfs is now an instance method**: Changed from `Self::process_list_pdfs(&params)` to `self.process_list_pdfs(&params)` for sandbox checking. Integration tests use `PdfServer::new().process_list_pdfs_public(&params)`.
8. **pdfium-render form field API**: `field_type()` returns `PdfFormFieldType` (not `Option`). `is_checked()` returns `Result<bool>`. `options().get(i)` returns `Result`. `opt.label()` returns `Option<&String>` (needs `.cloned()`).

## Test Coverage Improvements Needed

Current coverage: ~70% line coverage. Priority areas for improvement:

### High Priority (0% coverage)
- **main.rs** - CLI argument parsing (`--resource-dir`, `--help`, `--version`)
  - Test `parse_args()` with various argument combinations
  - Test `parse_env_dirs()` with `PDF_RESOURCE_DIRS` environment variable
  - Test duplicate removal logic

### Medium Priority (~70% coverage)
- **server.rs** - MCP Resources methods
  - `list_resources` - Test with configured directories, empty directories
  - `read_resource` - Test with valid/invalid URIs, security boundary checks
  - Error handling paths for various tools

- **source/resolver.rs** (~65%)
  - URL resolution (`resolve_url`)
  - Error cases for each source type

- **pdf/reader.rs** (~69%)
  - Edge cases in text extraction
  - Various PDF structures (multi-column, watermarks, etc.)

### Running Coverage

```bash
# Generate coverage report
docker compose --profile dev run --rm coverage

# View report
open coverage/html/index.html
```

## MCP Registry

The server is published to the [MCP Registry](https://registry.modelcontextprotocol.io/) as a Docker/OCI image on ghcr.io.

- **Registry name**: `io.github.paradyno/pdf-mcp-server`
- **Config file**: `server.json` (repo root)
- **Docker label**: `io.modelcontextprotocol.server.name` in Dockerfile (production stage)
- **Publish flow**: tag push (`v*`) → GitHub Actions (`release.yml`) → Docker push to ghcr.io → `mcp-publisher publish` (OIDC auth)

### Version Sync

`server.json` の `version` と `packages[].identifier` の Docker tag は、リリース時に GitHub Actions の `publish-mcp-registry` ジョブが git tag から自動で書き換える。そのため `server.json` のバージョンを手動で更新する必要はないが、**`server.json` をローカルで編集した場合はバージョン値を変えないよう注意すること**。CI が `jq` で上書きする。

## Future Improvements (Phase 3+)

Potential features to add:

- `rotate_pages` - Rotate specific pages
- `extract_tables` - Structured table extraction
- `add_watermark` - Text/image watermarks
- `linearize_pdf` - Web optimization
- `flatten_annotations` - Make annotations permanent
