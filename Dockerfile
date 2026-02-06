# Development Dockerfile for PDF MCP Server
# Includes all tools needed for building, testing, and coverage

FROM rust:1.93

# Install system dependencies
RUN apt-get update && apt-get install -y \
    # Build essentials
    build-essential \
    pkg-config \
    # PDFium dependencies
    libclang-dev \
    # qpdf
    qpdf \
    # For downloading PDFium
    curl \
    wget \
    # Code coverage
    llvm \
    # Useful tools
    git \
    && rm -rf /var/lib/apt/lists/*

# Install Rust components
RUN rustup component add clippy rustfmt

# Install cargo tools
RUN cargo install cargo-llvm-cov && \
    cargo install --locked cargo-nextest

# Download PDFium pre-built library from bblanchon/pdfium-binaries
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

# The actual source will be mounted via volume
CMD ["cargo", "build"]
