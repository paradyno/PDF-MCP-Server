# PDF MCP Server

A high-performance MCP (Model Context Protocol) server for PDF processing, built in Rust.

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![CI](https://github.com/paradyno/pdf-mcp-server/actions/workflows/ci.yml/badge.svg)](https://github.com/paradyno/pdf-mcp-server/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/paradyno/pdf-mcp-server/branch/main/graph/badge.svg)](https://codecov.io/gh/paradyno/pdf-mcp-server)

## Overview

PDF MCP Server provides AI agents with powerful PDF processing capabilities through the Model Context Protocol. It uses **PDFium** for text extraction and **qpdf** for PDF manipulation, ensuring a clean Apache 2.0 licensed solution.

### Key Features

- **Text Extraction** - Extract text from PDFs with page selection support
- **Metadata Extraction** - Get document properties (author, title, dates, etc.)
- **Outline Extraction** - Get PDF bookmarks/table of contents with page ranges
- **Annotation Extraction** - Extract highlights, comments, and other annotations
- **Image Extraction** - Extract embedded images from PDFs
- **Link Extraction** - Extract hyperlinks and internal page navigation
- **Page Info** - Get page dimensions, word/char counts, token estimates, and file sizes
- **Search** - Full-text search within PDFs with context
- **PDF Manipulation** - Split, merge, compress, protect, and unprotect PDFs
- **Batch Processing** - Process multiple PDFs in parallel
- **Caching** - Optional caching for repeated operations and chained operations
- **Password Support** - Handle password-protected PDFs

## Installation

### npm (Recommended)

```bash
npm install -g @paradyno/pdf-mcp-server
```

### Pre-built Binaries

Download the latest release for your platform from [GitHub Releases](https://github.com/paradyno/pdf-mcp-server/releases):

| Platform | Architecture | Download |
|----------|--------------|----------|
| Linux | x86_64 | `pdf-mcp-server-linux-x64` |
| Linux | ARM64 | `pdf-mcp-server-linux-arm64` |
| macOS | Intel | `pdf-mcp-server-darwin-x64` |
| macOS | Apple Silicon | `pdf-mcp-server-darwin-arm64` |
| Windows | x86_64 | `pdf-mcp-server-windows-x64.exe` |

### From Source

```bash
cargo install --git https://github.com/paradyno/pdf-mcp-server
```

## Configuration

### Claude Desktop

Add to your `claude_desktop_config.json`:

**macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`
**Windows:** `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "pdf": {
      "command": "npx",
      "args": ["@paradyno/pdf-mcp-server"]
    }
  }
}
```

Or if using a downloaded binary:

```json
{
  "mcpServers": {
    "pdf": {
      "command": "/path/to/pdf-mcp-server"
    }
  }
}
```

### Claude Code

```bash
claude mcp add pdf -- npx @paradyno/pdf-mcp-server
```

### VS Code (with MCP extension)

Add to your MCP settings:

```json
{
  "mcp.servers": {
    "pdf": {
      "command": "npx",
      "args": ["@paradyno/pdf-mcp-server"]
    }
  }
}
```

## Tools

### `extract_text`

Extract text content from PDF files.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `sources` | array | Yes | - | PDF sources (see Source Types below) |
| `pages` | string | No | all | Page selection (e.g., "1-5,10,15-20") |
| `include_metadata` | boolean | No | true | Include PDF metadata |
| `password` | string | No | - | PDF password if encrypted |
| `cache` | boolean | No | false | Enable caching |

**Example:**

```json
{
  "sources": [{ "path": "/documents/report.pdf" }],
  "pages": "1-10",
  "include_metadata": true
}
```

### `extract_outline`

Extract PDF bookmarks/table of contents.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `sources` | array | Yes | - | PDF sources |
| `password` | string | No | - | PDF password if encrypted |
| `cache` | boolean | No | false | Enable caching |

**Example:**

```json
{
  "sources": [{ "path": "/documents/book.pdf" }]
}
```

**Response:**

```json
{
  "results": [{
    "source": "/documents/book.pdf",
    "outline": [
      {
        "title": "Chapter 1: Introduction",
        "page": 1,
        "children": [
          { "title": "1.1 Background", "page": 3, "children": [] }
        ]
      }
    ]
  }]
}
```

### `search`

Search for text within PDF files.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `sources` | array | Yes | - | PDF sources |
| `query` | string | Yes | - | Search query |
| `case_sensitive` | boolean | No | false | Case-sensitive search |
| `max_results` | integer | No | 100 | Maximum results to return |
| `context_chars` | integer | No | 50 | Characters of context around match |
| `password` | string | No | - | PDF password if encrypted |
| `cache` | boolean | No | false | Enable caching |

**Example:**

```json
{
  "sources": [{ "path": "/documents/manual.pdf" }],
  "query": "error handling",
  "context_chars": 100
}
```

### `extract_metadata`

Extract PDF metadata without loading full content. Fast operation for getting document information.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `sources` | array | Yes | - | PDF sources |
| `password` | string | No | - | PDF password if encrypted |
| `cache` | boolean | No | false | Enable caching |

### `extract_annotations`

Extract annotations (highlights, comments, underlines, etc.) from PDF files.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `sources` | array | Yes | - | PDF sources |
| `annotation_types` | array | No | all | Filter by types (highlight, underline, text, etc.) |
| `pages` | string | No | all | Page selection |
| `password` | string | No | - | PDF password if encrypted |
| `cache` | boolean | No | false | Enable caching |

### `split_pdf`

Extract specific pages from a PDF to create a new PDF.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `source` | object | Yes | - | PDF source |
| `pages` | string | Yes | - | Page range (see Page Range Syntax) |
| `output_path` | string | No | - | Save output to file |
| `password` | string | No | - | PDF password if encrypted |

**Page Range Syntax:**

| Syntax | Description |
|--------|-------------|
| `1-5` | Pages 1 through 5 |
| `1,3,5` | Specific pages |
| `z` | Last page |
| `r1` | Last page (reverse notation) |
| `5-z` | Page 5 to end |
| `z-1` | All pages reversed |
| `1-z:odd` | Odd pages only |
| `1-z:even` | Even pages only |
| `1-10,x5` | Pages 1-10 except page 5 |

**Example:**

```json
{
  "source": { "path": "/documents/book.pdf" },
  "pages": "1-10,15,20-z",
  "output_path": "/output/excerpt.pdf"
}
```

### `merge_pdfs`

Merge multiple PDF files into a single PDF.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `sources` | array | Yes | - | PDF sources to merge (in order) |
| `output_path` | string | No | - | Save output to file |

**Example:**

```json
{
  "sources": [
    { "path": "/documents/chapter1.pdf" },
    { "path": "/documents/chapter2.pdf" },
    { "path": "/documents/chapter3.pdf" }
  ],
  "output_path": "/output/complete-book.pdf"
}
```

### `protect_pdf`

Add password protection to a PDF file using 256-bit AES encryption.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `source` | object | Yes | - | PDF source |
| `user_password` | string | Yes | - | Password to open the PDF |
| `owner_password` | string | No | user_password | Password to change permissions |
| `allow_print` | string | No | "full" | Print permission: "full", "low", or "none" |
| `allow_copy` | boolean | No | true | Allow copying text/images |
| `allow_modify` | boolean | No | true | Allow modifying the document |
| `output_path` | string | No | - | Save output to file |
| `password` | string | No | - | Password for source PDF if encrypted |

**Example:**

```json
{
  "source": { "path": "/documents/confidential.pdf" },
  "user_password": "secret123",
  "allow_print": "none",
  "allow_copy": false,
  "output_path": "/output/protected.pdf"
}
```

### `unprotect_pdf`

Remove password protection from an encrypted PDF.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `source` | object | Yes | - | PDF source |
| `password` | string | Yes | - | Password for the encrypted PDF |
| `output_path` | string | No | - | Save output to file |

**Example:**

```json
{
  "source": { "path": "/documents/protected.pdf" },
  "password": "secret123",
  "output_path": "/output/unprotected.pdf"
}
```

### `extract_links`

Extract hyperlinks from PDF files. Returns both external URLs and internal page navigation links.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `sources` | array | Yes | - | PDF sources |
| `pages` | string | No | all | Page selection |
| `password` | string | No | - | PDF password if encrypted |
| `cache` | boolean | No | false | Enable caching |

**Response:**

```json
{
  "results": [{
    "source": "/documents/paper.pdf",
    "links": [
      {
        "page": 1,
        "url": "https://example.com",
        "text": "Click here"
      },
      {
        "page": 3,
        "dest_page": 10,
        "text": "See Chapter 5"
      }
    ],
    "total_count": 2
  }]
}
```

### `get_page_info`

Get detailed information about each page in a PDF. Useful for planning LLM context usage.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `sources` | array | Yes | - | PDF sources |
| `password` | string | No | - | PDF password if encrypted |
| `cache` | boolean | No | false | Enable caching |
| `skip_file_sizes` | boolean | No | false | Skip file size calculation (faster) |

**Response:**

```json
{
  "results": [{
    "source": "/documents/report.pdf",
    "pages": [
      {
        "page": 1,
        "width": 612.0,
        "height": 792.0,
        "rotation": 0,
        "orientation": "portrait",
        "char_count": 2500,
        "word_count": 450,
        "estimated_token_count": 625,
        "file_size": 102400
      }
    ],
    "total_pages": 10,
    "total_chars": 25000,
    "total_words": 4500,
    "total_estimated_token_count": 6250
  }]
}
```

**Token Estimation Note:**

Token counts are **model-dependent approximations** for context window planning:
- Latin/English: ~4 characters per token
- CJK (Chinese/Japanese/Korean): ~2 tokens per character

Actual token counts vary by model (GPT, Claude, etc.). Use as rough guidance only.

**File Size:**

By default, the tool calculates actual file sizes by splitting each page (~16ms/page). This provides accurate size information including shared resources (fonts, images). Use `skip_file_sizes=true` to skip this calculation for faster performance.

### `compress_pdf`

Compress a PDF file to reduce its size using stream optimization, object deduplication, and image optimization.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `source` | object | Yes | - | PDF source |
| `object_streams` | string | No | "generate" | Object streams mode: "generate" (best), "preserve", "disable" |
| `compression_level` | integer | No | 9 | Compression level (1-9, higher = better compression) |
| `output_path` | string | No | - | Save output to file |
| `password` | string | No | - | Password for source PDF if encrypted |

**Example:**

```json
{
  "source": { "path": "/documents/large-report.pdf" },
  "compression_level": 9,
  "output_path": "/output/compressed.pdf"
}
```

**Response:**

```json
{
  "results": [{
    "source": "/documents/large-report.pdf",
    "output_cache_key": "abc123",
    "original_size": 5242880,
    "compressed_size": 2097152,
    "compression_ratio": 0.4,
    "bytes_saved": 3145728,
    "output_page_count": 50
  }]
}
```

## Source Types

PDF sources can be specified in multiple ways:

```json
// File path (absolute or relative)
{ "path": "/documents/file.pdf" }

// Base64 encoded PDF data
{ "base64": "JVBERi0xLjQK..." }

// URL (HTTP/HTTPS)
{ "url": "https://example.com/document.pdf" }

// Cache reference (from previous operation)
{ "cache_key": "abc123" }
```

## Caching

When `cache: true` is specified, the server returns a `cache_key` that can be used in subsequent requests:

```json
// First request
{
  "sources": [{ "path": "/documents/large.pdf" }],
  "cache": true
}

// Response includes cache_key
{
  "results": [{
    "cache_key": "a1b2c3d4",
    "..."
  }]
}

// Subsequent requests can use cache_key
{
  "sources": [{ "cache_key": "a1b2c3d4" }],
  "pages": "50-60"
}
```

## Development

### Prerequisites

- Docker (for local development without installing Rust)
- Or: Rust 1.75+ (if building natively)

### Using Docker (Recommended)

The project uses Docker Compose with profiles to separate development and production environments.

```bash
# Build
docker compose --profile dev run --rm dev cargo build

# Run tests
docker compose --profile dev run --rm test

# Run tests with coverage
docker compose --profile dev run --rm coverage

# Format code
docker compose --profile dev run --rm dev cargo fmt --all

# Lint
docker compose --profile dev run --rm clippy

# Build production image (minimal runtime, ~120MB)
docker compose --profile prod build production

# Clean up dev images when no longer needed
docker compose --profile dev down --rmi local
```

### Native Development

You need to install PDFium locally. Download from [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries/releases) and set the `PDFIUM_PATH` environment variable.

```bash
# Build
cargo build --release

# Run tests
cargo test

# Run with coverage
cargo llvm-cov --html
```

### Project Structure

```
pdf-mcp-server/
├── src/
│   ├── main.rs              # Entry point
│   ├── lib.rs               # Library root
│   ├── server.rs            # MCP server implementation
│   ├── error.rs             # Error types
│   ├── pdf/                 # PDF processing layer
│   │   ├── mod.rs
│   │   └── reader.rs        # PDFium wrapper
│   └── source/              # Source handling
│       ├── mod.rs
│       ├── resolver.rs      # Path/URL/Base64 resolution
│       └── cache.rs         # Caching layer
├── tests/                   # Integration tests
│   ├── fixtures/            # Test PDF files
│   └── integration_test.rs
├── npm/                     # npm wrapper package
├── Cargo.toml
├── Dockerfile
└── .github/
    └── workflows/
        ├── ci.yml           # CI pipeline
        └── release.yml      # Release builds
```

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                   MCP Server (rmcp)                 │
├─────────────────────────────────────────────────────┤
│  Tools: extract_text | extract_outline | search     │
├─────────────────────────────────────────────────────┤
│              Common Layer                           │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐           │
│  │  Cache   │  │  Source  │  │  Batch   │           │
│  │ Manager  │  │ Resolver │  │ Executor │           │
│  └──────────┘  └──────────┘  └──────────┘           │
├─────────────────────────────────────────────────────┤
│              PDF Processing Layer                   │
│  ┌───────────────────┐  ┌───────────────────┐       │
│  │   pdfium-render   │  │       qpdf        │       │
│  │   (reading)       │  │   (manipulation)  │       │
│  └───────────────────┘  └───────────────────┘       │
└─────────────────────────────────────────────────────┘
```

## Roadmap

### Phase 1: Core Reading Features (Complete)
- [x] Project setup
- [x] extract_text tool
- [x] extract_outline tool
- [x] search tool
- [x] extract_metadata tool
- [x] extract_annotations tool
- [x] Image extraction (include_images option)
- [x] Batch processing
- [x] Caching

### Phase 2: PDF Manipulation (Complete)
- [x] split_pdf - Extract specific pages
- [x] merge_pdfs - Merge multiple PDFs
- [x] protect_pdf - Add password protection (256-bit AES)
- [x] unprotect_pdf - Remove password protection
- [x] compress_pdf - Reduce file size with optimization
- [x] extract_links - Extract hyperlinks and internal links
- [x] get_page_info - Get page dimensions and token estimates

### Phase 3: Advanced Features (Planned)
- [ ] rotate_pages - Rotate specific pages
- [ ] convert_to_images - Render PDF pages as images (PNG/JPEG)
- [ ] extract_tables - Structured table data extraction
- [ ] add_watermark - Add text/image watermarks
- [ ] linearize_pdf - Web optimization for fast viewing
- [ ] flatten_annotations - Make annotations permanent
- [ ] reorder_pages - Rearrange page order
- [ ] delete_pages - Remove specific pages
- [ ] OCR support (optional, via external service)
- [ ] PDF/A validation
- [ ] Digital signature verification

## License

Apache License 2.0

## Acknowledgments

- [PDFium](https://pdfium.googlesource.com/pdfium/) - PDF rendering engine (Apache 2.0)
- [pdfium-render](https://crates.io/crates/pdfium-render) - Rust bindings for PDFium (Apache 2.0)
- [qpdf](https://qpdf.sourceforge.io/) - PDF transformation library (Apache 2.0)
- [rmcp](https://crates.io/crates/rmcp) - Rust MCP SDK
