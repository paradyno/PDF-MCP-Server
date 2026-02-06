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
│   └── qpdf.rs       # qpdf CLI wrapper (split, merge, encrypt, decrypt)
└── source/
    ├── mod.rs        # Source module exports
    ├── resolver.rs   # PdfSource resolution (path, base64, url, cache)
    └── cache.rs      # In-memory PDF cache (LRU)
```

## Key Technologies

- **rmcp**: Rust MCP framework for tool definitions
- **pdfium-render**: PDFium bindings for PDF reading
- **qpdf**: External CLI tool for PDF manipulation (must be installed)
- **tokio**: Async runtime
- **serde/schemars**: JSON serialization and schema generation

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

## Working with qpdf

### Important: Warning Handling

qpdf may exit with non-zero status code for warnings (e.g., "operation succeeded with warnings"). Always check stderr for success message:

```rust
let output = cmd.output()?;
let stderr = String::from_utf8_lossy(&output.stderr);

if !output.status.success() {
    // Check if it was actually a success with warnings
    if !stderr.contains("operation succeeded") {
        return Err(Error::QpdfError { reason: stderr.to_string() });
    }
}
```

### Common qpdf Commands

```bash
# Split pages
qpdf input.pdf --pages . 1-5,10 -- output.pdf

# Merge PDFs
qpdf --empty --pages file1.pdf 1-z file2.pdf 1-z -- output.pdf

# Encrypt (256-bit AES)
qpdf input.pdf --encrypt user-pass owner-pass 256 -- output.pdf

# Decrypt
qpdf --password=secret --decrypt input.pdf output.pdf

# Get page count
qpdf --show-npages input.pdf

# Compress/Optimize
qpdf input.pdf --object-streams=generate --recompress-flate \
  --compression-level=9 --optimize-images \
  --remove-unreferenced-resources=yes --normalize-content=y output.pdf
```

### Page Range Syntax

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

1. **qpdf warnings**: Always handle "operation succeeded with warnings" case
2. **Temp files**: Use `tempfile::NamedTempFile` for qpdf I/O
3. **Password errors**: Check stderr for "invalid password" patterns
4. **Page counts**: Use `QpdfWrapper::get_page_count()` for encrypted PDFs after operations
5. **Clippy**: Run before committing - the project uses strict linting
6. **Format**: Always run `cargo fmt` before committing

## Future Improvements (Phase 3+)

Potential features to add:

- `rotate_pages` - Rotate specific pages
- `add_overlay` - Add watermarks/stamps
- `optimize_pdf` - Reduce file size
- `linearize_pdf` - Web optimization
- `flatten_annotations` - Make annotations permanent
