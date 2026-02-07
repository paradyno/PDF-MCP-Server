//! PDF MCP Server - Entry point
//!
//! A high-performance MCP server for PDF processing.
//!
//! ## Resource Directory Configuration
//!
//! PDF files in configured directories are exposed as MCP resources.
//!
//! ### Environment Variable
//! ```bash
//! PDF_RESOURCE_DIRS=/documents:/data/pdfs pdf-mcp-server
//! ```
//!
//! ### Command Line Arguments
//! ```bash
//! pdf-mcp-server --resource-dir /documents --resource-dir /data/pdfs
//! ```
//!
//! Both methods can be combined. Command line arguments are added to
//! environment variable paths.

use pdf_mcp_server::{run_server_with_config, ServerConfig};
use std::env;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Parse command line arguments into a ServerConfig
fn parse_args() -> ServerConfig {
    let args: Vec<String> = env::args().collect();
    let mut config = parse_env_config();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--resource-dir" | "-r" => {
                if i + 1 < args.len() {
                    config.resource_dirs.push(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("Error: --resource-dir requires a path argument");
                    std::process::exit(1);
                }
            }
            "--allow-private-urls" => {
                config.allow_private_urls = true;
                i += 1;
            }
            "--max-download-size" => {
                if i + 1 < args.len() {
                    config.max_download_bytes = args[i + 1].parse().unwrap_or_else(|_| {
                        eprintln!("Error: --max-download-size requires a numeric argument");
                        std::process::exit(1);
                    });
                    i += 2;
                } else {
                    eprintln!("Error: --max-download-size requires a numeric argument");
                    std::process::exit(1);
                }
            }
            "--cache-max-bytes" => {
                if i + 1 < args.len() {
                    config.cache_max_bytes = args[i + 1].parse().unwrap_or_else(|_| {
                        eprintln!("Error: --cache-max-bytes requires a numeric argument");
                        std::process::exit(1);
                    });
                    i += 2;
                } else {
                    eprintln!("Error: --cache-max-bytes requires a numeric argument");
                    std::process::exit(1);
                }
            }
            "--cache-max-entries" => {
                if i + 1 < args.len() {
                    config.cache_max_entries = args[i + 1].parse().unwrap_or_else(|_| {
                        eprintln!("Error: --cache-max-entries requires a numeric argument");
                        std::process::exit(1);
                    });
                    i += 2;
                } else {
                    eprintln!("Error: --cache-max-entries requires a numeric argument");
                    std::process::exit(1);
                }
            }
            "--max-image-scale" => {
                if i + 1 < args.len() {
                    config.max_image_scale = args[i + 1].parse().unwrap_or_else(|_| {
                        eprintln!("Error: --max-image-scale requires a numeric argument");
                        std::process::exit(1);
                    });
                    i += 2;
                } else {
                    eprintln!("Error: --max-image-scale requires a numeric argument");
                    std::process::exit(1);
                }
            }
            "--max-image-pixels" => {
                if i + 1 < args.len() {
                    config.max_image_pixels = args[i + 1].parse().unwrap_or_else(|_| {
                        eprintln!("Error: --max-image-pixels requires a numeric argument");
                        std::process::exit(1);
                    });
                    i += 2;
                } else {
                    eprintln!("Error: --max-image-pixels requires a numeric argument");
                    std::process::exit(1);
                }
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("pdf-mcp-server {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            arg if arg.starts_with("--resource-dir=") => {
                if let Some(path) = arg.strip_prefix("--resource-dir=") {
                    config.resource_dirs.push(path.to_string());
                }
                i += 1;
            }
            arg if arg.starts_with("-r=") => {
                if let Some(path) = arg.strip_prefix("-r=") {
                    config.resource_dirs.push(path.to_string());
                }
                i += 1;
            }
            arg => {
                eprintln!("Unknown argument: {}", arg);
                eprintln!("Use --help for usage information");
                std::process::exit(1);
            }
        }
    }

    config
}

/// Parse server configuration from environment variables
fn parse_env_config() -> ServerConfig {
    let resource_dirs: Vec<String> = env::var_os("PDF_RESOURCE_DIRS")
        .map(|val| {
            env::split_paths(&val)
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();

    let allow_private_urls = env::var("PDF_ALLOW_PRIVATE_URLS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let max_download_bytes = env::var("PDF_MAX_DOWNLOAD_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100 * 1024 * 1024);

    let cache_max_bytes = env::var("PDF_CACHE_MAX_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(512 * 1024 * 1024);

    let cache_max_entries = env::var("PDF_CACHE_MAX_ENTRIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);

    let max_image_scale = env::var("PDF_MAX_IMAGE_SCALE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10.0);

    let max_image_pixels = env::var("PDF_MAX_IMAGE_PIXELS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000_000);

    ServerConfig {
        resource_dirs,
        allow_private_urls,
        max_download_bytes,
        cache_max_bytes,
        cache_max_entries,
        max_image_scale,
        max_image_pixels,
    }
}

fn print_help() {
    println!(
        r#"PDF MCP Server - A high-performance MCP server for PDF processing

USAGE:
    pdf-mcp-server [OPTIONS]

OPTIONS:
    -r, --resource-dir <PATH>        Add a directory to expose as PDF resources
                                     Can be specified multiple times.
                                     When set, file access is sandboxed to these directories.
        --allow-private-urls         Allow URL sources that resolve to private/reserved IPs
        --max-download-size <BYTES>  Maximum URL download size in bytes (default: 104857600 = 100MB)
        --cache-max-bytes <BYTES>    Maximum total cache size in bytes (default: 536870912 = 512MB)
        --cache-max-entries <N>      Maximum number of cache entries (default: 100)
        --max-image-scale <SCALE>    Maximum scale factor for page rendering (default: 10.0)
        --max-image-pixels <N>       Maximum pixel area for page rendering (default: 100000000)
    -h, --help                       Print help information
    -V, --version                    Print version information

ENVIRONMENT VARIABLES:
    PDF_RESOURCE_DIRS            Path-separated list of resource directories
                                 (colon-separated on Unix, semicolon-separated on Windows)
                                 Example: /documents:/data/pdfs
    PDF_ALLOW_PRIVATE_URLS       Set to "1" or "true" to allow private URLs
    PDF_MAX_DOWNLOAD_BYTES       Maximum URL download size in bytes
    PDF_CACHE_MAX_BYTES          Maximum total cache size in bytes
    PDF_CACHE_MAX_ENTRIES        Maximum number of cache entries
    PDF_MAX_IMAGE_SCALE          Maximum scale factor for page rendering
    PDF_MAX_IMAGE_PIXELS         Maximum pixel area for page rendering

SECURITY:
    When --resource-dir is specified, all file operations (reads and writes) are
    sandboxed to the configured directories. Path traversal attempts are blocked.

    URL sources are checked for SSRF by default. URLs that resolve to private or
    reserved IP addresses (loopback, link-local, RFC1918, CGNAT) are blocked.
    Use --allow-private-urls to disable this check.

EXAMPLES:
    # Start server without resources
    pdf-mcp-server

    # Start server with resource directories (enables sandboxing)
    pdf-mcp-server --resource-dir /documents --resource-dir /data/pdfs

    # Using environment variable
    PDF_RESOURCE_DIRS=/documents:/data/pdfs pdf-mcp-server

    # Combine both (directories are merged)
    PDF_RESOURCE_DIRS=/documents pdf-mcp-server --resource-dir /data/pdfs

    # Allow private URLs (e.g., for local development)
    pdf-mcp-server --allow-private-urls
"#
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pdf_mcp_server=info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    // Parse config from env and command line (CLI overrides env)
    let mut config = parse_args();

    // Remove duplicates while preserving order
    let mut seen = std::collections::HashSet::new();
    config.resource_dirs.retain(|dir| seen.insert(dir.clone()));

    if config.resource_dirs.is_empty() {
        tracing::info!("Starting PDF MCP Server (no resource directories configured)");
    } else {
        tracing::info!(
            "Starting PDF MCP Server with resource directories: {:?}",
            config.resource_dirs
        );
    }

    run_server_with_config(config).await
}
