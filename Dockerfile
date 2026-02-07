# Multi-stage Dockerfile for PDF MCP Server
# Usage:
#   target=dev        -> Development (build tools, testing, coverage)
#   target=production -> Minimal runtime image

# ============================================================
# Stage 1: base - common build dependencies and PDFium
# ============================================================
FROM rust:1.93 AS base

RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libclang-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

ARG PDFIUM_VERSION=7651
RUN mkdir -p /opt/pdfium && \
    ARCH=$(uname -m) && \
    if [ "$ARCH" = "x86_64" ]; then \
        curl -L "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F${PDFIUM_VERSION}/pdfium-linux-x64.tgz" -o /tmp/pdfium.tgz && \
        tar -xzf /tmp/pdfium.tgz -C /opt/pdfium && rm /tmp/pdfium.tgz; \
    elif [ "$ARCH" = "aarch64" ]; then \
        curl -L "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F${PDFIUM_VERSION}/pdfium-linux-arm64.tgz" -o /tmp/pdfium.tgz && \
        tar -xzf /tmp/pdfium.tgz -C /opt/pdfium && rm /tmp/pdfium.tgz; \
    fi

ENV PDFIUM_PATH=/opt/pdfium

WORKDIR /app

# ============================================================
# Stage 2: dev - development tools for building, testing, coverage
# ============================================================
FROM base AS dev

RUN apt-get update && apt-get install -y \
    wget \
    llvm \
    git \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add clippy rustfmt

RUN cargo install cargo-llvm-cov && \
    cargo install --locked cargo-nextest

# The actual source will be mounted via volume
CMD ["cargo", "build"]

# ============================================================
# Stage 3: builder - compile release binary
# ============================================================
FROM base AS builder

# Cache dependencies by building with dummy source first
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs && \
    mkdir benches && echo 'fn main() {}' > benches/pdf_benchmark.rs && \
    cargo build --release && \
    rm -rf src benches

# Build the actual application
COPY src/ src/
RUN touch src/main.rs src/lib.rs && cargo build --release

# ============================================================
# Stage 4: production - minimal runtime image
# ============================================================
FROM debian:trixie-slim AS production

LABEL io.modelcontextprotocol.server.name="io.github.paradyno/pdf-mcp-server"

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy PDFium shared library
COPY --from=builder /opt/pdfium/lib/ /opt/pdfium/lib/
ENV PDFIUM_PATH=/opt/pdfium
ENV LD_LIBRARY_PATH=/opt/pdfium/lib

# Copy the compiled binary (qpdf is statically linked via vendored FFI)
COPY --from=builder /app/target/release/pdf-mcp-server /usr/local/bin/pdf-mcp-server

RUN useradd --create-home --shell /bin/sh appuser
USER appuser

ENTRYPOINT ["pdf-mcp-server"]
