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

use pdf_mcp_server::run_server_with_dirs;
use std::env;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Parse command line arguments for resource directories
fn parse_args() -> Vec<String> {
    let args: Vec<String> = env::args().collect();
    let mut resource_dirs = Vec::new();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--resource-dir" | "-r" => {
                if i + 1 < args.len() {
                    resource_dirs.push(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("Error: --resource-dir requires a path argument");
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
                    resource_dirs.push(path.to_string());
                }
                i += 1;
            }
            arg if arg.starts_with("-r=") => {
                if let Some(path) = arg.strip_prefix("-r=") {
                    resource_dirs.push(path.to_string());
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

    resource_dirs
}

/// Parse resource directories from environment variable
fn parse_env_dirs() -> Vec<String> {
    env::var("PDF_RESOURCE_DIRS")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn print_help() {
    println!(
        r#"PDF MCP Server - A high-performance MCP server for PDF processing

USAGE:
    pdf-mcp-server [OPTIONS]

OPTIONS:
    -r, --resource-dir <PATH>    Add a directory to expose as PDF resources
                                 Can be specified multiple times
    -h, --help                   Print help information
    -V, --version                Print version information

ENVIRONMENT VARIABLES:
    PDF_RESOURCE_DIRS            Colon-separated list of resource directories
                                 Example: /documents:/data/pdfs

EXAMPLES:
    # Start server without resources
    pdf-mcp-server

    # Start server with resource directories
    pdf-mcp-server --resource-dir /documents --resource-dir /data/pdfs

    # Using environment variable
    PDF_RESOURCE_DIRS=/documents:/data/pdfs pdf-mcp-server

    # Combine both (directories are merged)
    PDF_RESOURCE_DIRS=/documents pdf-mcp-server --resource-dir /data/pdfs
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

    // Parse resource directories from env and command line
    let mut resource_dirs = parse_env_dirs();
    resource_dirs.extend(parse_args());

    // Remove duplicates while preserving order
    let mut seen = std::collections::HashSet::new();
    resource_dirs.retain(|dir| seen.insert(dir.clone()));

    if resource_dirs.is_empty() {
        tracing::info!("Starting PDF MCP Server (no resource directories configured)");
    } else {
        tracing::info!(
            "Starting PDF MCP Server with resource directories: {:?}",
            resource_dirs
        );
    }

    run_server_with_dirs(resource_dirs).await
}
