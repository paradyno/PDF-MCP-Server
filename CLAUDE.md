# CLAUDE.md - PDF MCP Server Development Guide

## Rules

- After making any changes to the project (code, configuration, build system, commands, etc.), always check whether `README.md` and `CLAUDE.md` need to be updated to reflect those changes. If updates are needed, propose the changes to the user.

## Project Overview

This is a Model Context Protocol (MCP) server for PDF operations, implemented in Rust. It provides tools for extracting text, metadata, annotations, images, and links from PDFs, as well as manipulation operations like splitting, merging, compression, and password protection.

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

# Environment variable (colon-separated)
PDF_RESOURCE_DIRS=/documents:/data/pdfs pdf-mcp-server
```

### Implementation

Resources are implemented via `ServerHandler` trait methods in `src/server.rs`:
- `list_resources`: Lists all PDFs in configured `resource_dirs`
- `read_resource`: Extracts text content from a PDF by `file://` URI

Resource directories are stored in `PdfServer::resource_dirs` and configured via:
- `PdfServer::with_resource_dirs(dirs)` - programmatic
- `run_server_with_dirs(dirs)` - server startup function

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
        .unwrap_or_else(|e| MyToolResult {
            source: Self::source_name(&params.source),
            output_cache_key: String::new(),
            error: Some(e.to_string()),
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
    let output_cache_key = CacheManager::generate_key();
    self.cache.write().await.put(output_cache_key.clone(), output_data);

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
    // ...
}
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

## Common Patterns

### Always Cache Output for Chaining

```rust
let output_cache_key = CacheManager::generate_key();
self.cache.write().await.put(output_cache_key.clone(), output_data.clone());
```

### Optional File Output

```rust
let output_path = if let Some(ref path_str) = params.output_path {
    let path = Path::new(path_str);
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

1. **qpdf FFI is sync**: All qpdf operations block. This matches the existing usage pattern in async handlers.
2. **Page indices**: qpdf crate uses 0-indexed pages, but our API uses 1-indexed. `parse_qpdf_page_range()` handles the conversion.
3. **Password errors**: qpdf crate returns `QPdfErrorCode::InvalidPassword` — mapped to `Error::IncorrectPassword` via `map_qpdf_error()`.
4. **Page counts**: Use `QpdfWrapper::get_page_count()` for encrypted PDFs after operations.
5. **Clippy**: Run before committing - the project uses strict linting.
6. **Format**: Always run `cargo fmt` before committing.

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

## Future Improvements (Phase 3+)

Potential features to add:

- `rotate_pages` - Rotate specific pages
- `add_overlay` - Add watermarks/stamps
- `optimize_pdf` - Reduce file size
- `linearize_pdf` - Web optimization
- `flatten_annotations` - Make annotations permanent
