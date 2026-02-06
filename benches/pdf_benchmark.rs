//! Performance benchmarks for PDF MCP Server
//!
//! Run with: `cargo bench`
//! Or via Docker: `docker compose --profile dev run --rm dev cargo bench`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use pdf_mcp_server::pdf::{
    extract_annotations, extract_images, extract_links, get_page_info, PdfReader,
};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    path.push(name);
    path
}

fn load_fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixture_path(name)).expect("Failed to read fixture")
}

/// Benchmark text extraction from a single PDF
fn bench_text_extraction(c: &mut Criterion) {
    let data = load_fixture("tracemonkey.pdf");

    let mut group = c.benchmark_group("text_extraction");
    group.throughput(Throughput::Bytes(data.len() as u64));

    group.bench_function("tracemonkey_14pages", |b| {
        b.iter(|| {
            let reader = PdfReader::open_bytes(black_box(&data), None).unwrap();
            let _ = reader.extract_all_text().unwrap();
        });
    });

    // Smaller PDF
    let small_data = load_fixture("dummy.pdf");
    group.throughput(Throughput::Bytes(small_data.len() as u64));

    group.bench_function("dummy_1page", |b| {
        b.iter(|| {
            let reader = PdfReader::open_bytes(black_box(&small_data), None).unwrap();
            let _ = reader.extract_all_text().unwrap();
        });
    });

    group.finish();
}

/// Benchmark processing multiple PDFs (simulates batch processing)
fn bench_batch_processing(c: &mut Criterion) {
    let files = [
        "dummy.pdf",
        "tracemonkey.pdf",
        "basicapi.pdf",
        "test-with-outline-and-images.pdf",
    ];

    // Load all files into memory
    let pdfs: Vec<Vec<u8>> = files.iter().map(|f| load_fixture(f)).collect();
    let total_bytes: usize = pdfs.iter().map(|p| p.len()).sum();

    let mut group = c.benchmark_group("batch_processing");
    group.throughput(Throughput::Elements(files.len() as u64));

    // Process N files
    for count in [4, 20, 100] {
        group.bench_with_input(
            BenchmarkId::new("extract_text", format!("{}_files", count)),
            &count,
            |b, &count| {
                b.iter(|| {
                    for i in 0..count {
                        let pdf = &pdfs[i % pdfs.len()];
                        let reader = PdfReader::open_bytes(black_box(pdf), None).unwrap();
                        let _ = reader.extract_all_text().unwrap();
                    }
                });
            },
        );
    }

    group.finish();

    // Report throughput
    println!(
        "\nBatch processing uses {} fixture files ({} KB total)",
        files.len(),
        total_bytes / 1024
    );
}

/// Benchmark metadata extraction (should be fast)
fn bench_metadata_extraction(c: &mut Criterion) {
    let data = load_fixture("tracemonkey.pdf");

    c.bench_function("metadata_extraction", |b| {
        b.iter(|| {
            let reader = PdfReader::open_bytes_metadata_only(black_box(&data), None).unwrap();
            let _ = reader.metadata();
            let _ = reader.page_count();
        });
    });
}

/// Benchmark page info extraction
fn bench_page_info(c: &mut Criterion) {
    let data = load_fixture("tracemonkey.pdf");

    c.bench_function("page_info_14pages", |b| {
        b.iter(|| {
            let _ = get_page_info(black_box(&data), None).unwrap();
        });
    });
}

/// Benchmark image extraction
fn bench_image_extraction(c: &mut Criterion) {
    let data = load_fixture("test-with-outline-and-images.pdf");

    c.bench_function("image_extraction", |b| {
        b.iter(|| {
            let _ = extract_images(black_box(&data), None).unwrap();
        });
    });
}

/// Benchmark annotation extraction
fn bench_annotation_extraction(c: &mut Criterion) {
    let data = load_fixture("tracemonkey.pdf");

    c.bench_function("annotation_extraction", |b| {
        b.iter(|| {
            let _ = extract_annotations(black_box(&data), None, None, None).unwrap();
        });
    });
}

/// Benchmark link extraction
fn bench_link_extraction(c: &mut Criterion) {
    let data = load_fixture("tracemonkey.pdf");

    c.bench_function("link_extraction", |b| {
        b.iter(|| {
            let _ = extract_links(black_box(&data), None, None).unwrap();
        });
    });
}

/// Benchmark search functionality
fn bench_search(c: &mut Criterion) {
    let data = load_fixture("tracemonkey.pdf");
    let reader = PdfReader::open_bytes(&data, None).unwrap();

    let mut group = c.benchmark_group("search");

    group.bench_function("case_insensitive", |b| {
        b.iter(|| {
            let _ = reader.search(black_box("JavaScript"), false);
        });
    });

    group.bench_function("case_sensitive", |b| {
        b.iter(|| {
            let _ = reader.search(black_box("JavaScript"), true);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_text_extraction,
    bench_batch_processing,
    bench_metadata_extraction,
    bench_page_info,
    bench_image_extraction,
    bench_annotation_extraction,
    bench_link_extraction,
    bench_search,
);

criterion_main!(benches);
