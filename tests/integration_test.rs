//! Integration tests for PDF MCP Server

use base64::Engine;
use pdf_mcp_server::pdf::{
    extract_annotations, extract_form_fields, extract_images, extract_images_from_pages,
    extract_links, fill_form_fields, get_page_info, parse_page_range, render_pages_to_images,
    FormFieldValue, PdfReader,
};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("fixtures");
    path.push(name);
    path
}

#[test]
fn test_open_dummy_pdf() {
    let path = fixture_path("dummy.pdf");
    let reader = PdfReader::open(&path, None).expect("Failed to open dummy.pdf");

    assert!(reader.page_count() > 0, "PDF should have at least one page");
}

#[test]
fn test_extract_text_from_dummy_pdf() {
    let path = fixture_path("dummy.pdf");
    let reader = PdfReader::open(&path, None).expect("Failed to open dummy.pdf");

    let text = reader
        .extract_page_text(1)
        .expect("Failed to extract text from page 1");
    // Dummy PDF should contain some text
    assert!(!text.is_empty() || reader.page_count() > 0);
}

#[test]
fn test_extract_all_text() {
    let path = fixture_path("dummy.pdf");
    let reader = PdfReader::open(&path, None).expect("Failed to open dummy.pdf");

    let all_text = reader
        .extract_all_text()
        .expect("Failed to extract all text");
    assert_eq!(all_text.len(), reader.page_count() as usize);
}

#[test]
fn test_metadata_extraction() {
    let path = fixture_path("dummy.pdf");
    let reader = PdfReader::open(&path, None).expect("Failed to open dummy.pdf");

    let metadata = reader.metadata();
    // Metadata fields can be None, but the struct should be accessible
    let _ = metadata.title.as_ref();
    let _ = metadata.author.as_ref();
    let _ = metadata.creator.as_ref();
}

#[test]
fn test_outline_extraction() {
    let path = fixture_path("test-with-outline-and-images.pdf");
    let reader = PdfReader::open(&path, None).expect("Failed to open test PDF");

    let outline = reader.get_outline();
    // This PDF should have an outline
    // Even if empty, the method should not fail
    let _ = outline;
}

#[test]
fn test_search_functionality() {
    let path = fixture_path("tracemonkey.pdf");
    let reader = PdfReader::open(&path, None).expect("Failed to open tracemonkey.pdf");

    // TracemonKey is a JavaScript paper, should contain "JavaScript"
    let matches = reader.search("JavaScript", false);
    // The search should work, even if no matches found
    let _ = matches;
}

#[test]
fn test_case_insensitive_search() {
    let path = fixture_path("tracemonkey.pdf");
    let reader = PdfReader::open(&path, None).expect("Failed to open tracemonkey.pdf");

    let matches_lower = reader.search("javascript", false);
    let matches_exact = reader.search("JavaScript", true);

    // Case-insensitive search should find at least as many matches
    assert!(matches_lower.len() >= matches_exact.len());
}

#[test]
fn test_page_range_with_pdf() {
    let path = fixture_path("tracemonkey.pdf");
    let reader = PdfReader::open(&path, None).expect("Failed to open tracemonkey.pdf");

    let page_count = reader.page_count();
    if page_count >= 3 {
        let pages = parse_page_range("1-3", page_count).expect("Failed to parse page range");
        let texts = reader
            .extract_pages_text(&pages)
            .expect("Failed to extract pages");
        assert_eq!(texts.len(), 3);
    }
}

#[test]
fn test_open_nonexistent_file() {
    let result = PdfReader::open("/nonexistent/path/file.pdf", None);
    assert!(result.is_err());
}

#[test]
fn test_open_invalid_pdf_bytes() {
    let result = PdfReader::open_bytes(b"not a valid PDF file", None);
    assert!(result.is_err());
}

#[test]
fn test_page_out_of_bounds() {
    let path = fixture_path("dummy.pdf");
    let reader = PdfReader::open(&path, None).expect("Failed to open dummy.pdf");

    let result = reader.extract_page_text(9999);
    assert!(result.is_err());
}

#[test]
fn test_basicapi_pdf() {
    let path = fixture_path("basicapi.pdf");
    let reader = PdfReader::open(&path, None).expect("Failed to open basicapi.pdf");

    assert!(reader.page_count() > 0);
    let text = reader.extract_page_text(1).expect("Failed to extract text");
    // Just verify extraction works
    let _ = text;
}

// ============================================================================
// Password-protected PDF tests
// ============================================================================

/// Test opening a password-protected PDF with the correct password
#[test]
fn test_password_protected_pdf_with_correct_password() {
    let path = fixture_path("password-protected.pdf");

    // Open with correct password (testpass)
    let reader = PdfReader::open(&path, Some("testpass"))
        .expect("Failed to open password-protected PDF with correct password");

    assert!(reader.page_count() > 0, "PDF should have at least one page");

    // Should be able to extract text
    let text = reader.extract_page_text(1).expect("Failed to extract text");
    let _ = text;
}

/// Test that opening a password-protected PDF without password fails
#[test]
fn test_password_protected_pdf_without_password() {
    let path = fixture_path("password-protected.pdf");

    // Open without password should fail
    let result = PdfReader::open(&path, None);
    assert!(
        result.is_err(),
        "Opening password-protected PDF without password should fail"
    );
}

/// Test that opening a password-protected PDF with wrong password fails
#[test]
fn test_password_protected_pdf_with_wrong_password() {
    let path = fixture_path("password-protected.pdf");

    // Open with wrong password should fail
    let result = PdfReader::open(&path, Some("wrongpassword"));
    assert!(
        result.is_err(),
        "Opening password-protected PDF with wrong password should fail"
    );
}

/// Test text extraction from password-protected PDF
#[test]
fn test_password_protected_pdf_text_extraction() {
    let path = fixture_path("password-protected.pdf");

    let reader =
        PdfReader::open(&path, Some("testpass")).expect("Failed to open password-protected PDF");

    // Extract all text
    let all_text = reader.extract_all_text().expect("Failed to extract text");
    assert_eq!(all_text.len(), reader.page_count() as usize);
}

/// Test search in password-protected PDF
#[test]
fn test_password_protected_pdf_search() {
    let path = fixture_path("password-protected.pdf");

    let reader =
        PdfReader::open(&path, Some("testpass")).expect("Failed to open password-protected PDF");

    // Search should work
    let matches = reader.search("dummy", false);
    let _ = matches;
}

/// Test metadata extraction from password-protected PDF
#[test]
fn test_password_protected_pdf_metadata() {
    let path = fixture_path("password-protected.pdf");

    let reader =
        PdfReader::open(&path, Some("testpass")).expect("Failed to open password-protected PDF");

    let metadata = reader.metadata();
    // Metadata should be accessible
    let _ = metadata.title.as_ref();
}

/// Test outline extraction from password-protected PDF
#[test]
fn test_password_protected_pdf_outline() {
    let path = fixture_path("password-protected.pdf");

    let reader =
        PdfReader::open(&path, Some("testpass")).expect("Failed to open password-protected PDF");

    let outline = reader.get_outline();
    let _ = outline;
}

// ============================================================================
// Image extraction tests
// ============================================================================

/// Test extracting all images from a PDF with images
#[test]
fn test_extract_images_from_pdf() {
    let path = fixture_path("test-with-outline-and-images.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let images = extract_images(&data, None).expect("Failed to extract images");

    // The test PDF should have at least one image
    // Even if it doesn't, the function should not fail
    for image in &images {
        assert!(image.page > 0, "Page number should be 1-indexed");
        assert!(image.width > 0, "Image width should be positive");
        assert!(image.height > 0, "Image height should be positive");
        assert!(
            !image.data_base64.is_empty(),
            "Image data should not be empty"
        );
        assert_eq!(image.mime_type, "image/png", "MIME type should be PNG");
    }
}

/// Test extracting images from specific pages
#[test]
fn test_extract_images_from_specific_pages() {
    let path = fixture_path("test-with-outline-and-images.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Extract images only from page 1
    let images = extract_images_from_pages(&data, None, &[1]).expect("Failed to extract images");

    // All extracted images should be from page 1
    for image in &images {
        assert_eq!(image.page, 1, "All images should be from page 1");
    }
}

/// Test extracting images from an empty page range
#[test]
fn test_extract_images_empty_pages() {
    let path = fixture_path("test-with-outline-and-images.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Empty page list should return empty result
    let images = extract_images_from_pages(&data, None, &[]).expect("Failed to extract images");
    assert!(images.is_empty(), "Empty page list should return no images");
}

/// Test extracting images from invalid PDF data
#[test]
fn test_extract_images_invalid_pdf() {
    let result = extract_images(b"not a valid PDF", None);
    assert!(result.is_err(), "Should fail for invalid PDF data");
}

/// Test that extracted image data is valid base64
#[test]
fn test_extracted_image_valid_base64() {
    let path = fixture_path("test-with-outline-and-images.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let images = extract_images(&data, None).expect("Failed to extract images");

    // If there are images, verify they decode as valid base64
    for image in &images {
        let decoded = base64::engine::general_purpose::STANDARD.decode(&image.data_base64);
        assert!(decoded.is_ok(), "Image data should be valid base64");

        // Verify PNG header (first 8 bytes)
        let decoded_data = decoded.unwrap();
        if decoded_data.len() >= 8 {
            assert_eq!(
                &decoded_data[0..8],
                &[137, 80, 78, 71, 13, 10, 26, 10],
                "Decoded data should have PNG header"
            );
        }
    }
}

// ============================================================================
// Metadata-only extraction tests
// ============================================================================

/// Test metadata-only extraction (fast path)
#[test]
fn test_metadata_only_extraction() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let reader =
        PdfReader::open_bytes_metadata_only(&data, None).expect("Failed to open PDF for metadata");

    // Should have page count
    assert!(reader.page_count() > 0, "PDF should have pages");

    // Metadata should be accessible
    let metadata = reader.metadata();
    let _ = metadata.title.as_ref();
    let _ = metadata.author.as_ref();
}

/// Test metadata-only extraction with password-protected PDF
#[test]
fn test_metadata_only_extraction_password_protected() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let reader = PdfReader::open_bytes_metadata_only(&data, Some("testpass"))
        .expect("Failed to open password-protected PDF for metadata");

    assert!(reader.page_count() > 0, "PDF should have pages");
}

/// Test metadata-only extraction without password fails
#[test]
fn test_metadata_only_extraction_password_required() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = PdfReader::open_bytes_metadata_only(&data, None);
    assert!(
        result.is_err(),
        "Opening password-protected PDF without password should fail"
    );
}

// ============================================================================
// Y-coordinate text ordering tests
// ============================================================================

/// Test that text extraction preserves reading order
#[test]
fn test_text_extraction_reading_order() {
    let path = fixture_path("tracemonkey.pdf");
    let reader = PdfReader::open(&path, None).expect("Failed to open tracemonkey.pdf");

    let text = reader
        .extract_page_text(1)
        .expect("Failed to extract text from page 1");

    // Text should not be empty
    assert!(!text.is_empty(), "Extracted text should not be empty");

    // Text should contain newlines (indicating line separation)
    assert!(
        text.contains('\n'),
        "Text should have newline separators between lines"
    );
}

/// Test text extraction with multi-column PDF (if available)
#[test]
fn test_text_extraction_layout_preservation() {
    let path = fixture_path("tracemonkey.pdf");
    let reader = PdfReader::open(&path, None).expect("Failed to open tracemonkey.pdf");

    // Extract first page text
    let text = reader
        .extract_page_text(1)
        .expect("Failed to extract text from page 1");

    // Verify text has structure (lines)
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines.len() > 1,
        "Text should have multiple lines, got {} lines",
        lines.len()
    );
}

// ============================================================================
// Annotation extraction tests
// ============================================================================

/// Test extracting annotations from a PDF (function should work even if no annotations exist)
#[test]
fn test_extract_annotations_basic() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Should not fail even if there are no annotations
    let annotations =
        extract_annotations(&data, None, None, None).expect("Failed to extract annotations");

    // Annotations list should be a valid vector (may be empty)
    let _ = annotations.len();
}

/// Test extracting annotations from specific pages
#[test]
fn test_extract_annotations_specific_pages() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Extract annotations only from page 1
    let annotations =
        extract_annotations(&data, None, Some(&[1]), None).expect("Failed to extract annotations");

    // All extracted annotations should be from page 1
    for annotation in &annotations {
        assert_eq!(annotation.page, 1, "All annotations should be from page 1");
    }
}

/// Test extracting annotations with type filter
#[test]
fn test_extract_annotations_type_filter() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Filter for only highlight annotations
    let types = vec!["highlight".to_string()];
    let annotations = extract_annotations(&data, None, None, Some(&types))
        .expect("Failed to extract annotations");

    // All extracted annotations should be highlight type
    for annotation in &annotations {
        assert_eq!(
            annotation.annotation_type, "highlight",
            "All annotations should be highlights"
        );
    }
}

/// Test extracting annotations from invalid PDF data
#[test]
fn test_extract_annotations_invalid_pdf() {
    let result = extract_annotations(b"not a valid PDF", None, None, None);
    assert!(result.is_err(), "Should fail for invalid PDF data");
}

/// Test extracting annotations from empty page list
#[test]
fn test_extract_annotations_empty_pages() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Empty page list should return empty result
    let annotations =
        extract_annotations(&data, None, Some(&[]), None).expect("Failed to extract annotations");
    assert!(
        annotations.is_empty(),
        "Empty page list should return no annotations"
    );
}

/// Test extracting annotations from password-protected PDF
#[test]
fn test_extract_annotations_password_protected() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Should work with correct password
    let annotations = extract_annotations(&data, Some("testpass"), None, None)
        .expect("Failed to extract annotations from password-protected PDF");
    let _ = annotations.len();
}

/// Test extracting annotations from password-protected PDF without password fails
#[test]
fn test_extract_annotations_password_required() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = extract_annotations(&data, None, None, None);
    assert!(
        result.is_err(),
        "Opening password-protected PDF without password should fail"
    );
}

/// Test annotation structure has required fields
#[test]
fn test_extract_annotations_structure() {
    let path = fixture_path("test-with-outline-and-images.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let annotations =
        extract_annotations(&data, None, None, None).expect("Failed to extract annotations");

    // Verify each annotation has required fields
    for annotation in &annotations {
        assert!(annotation.page > 0, "Page number should be positive");
        assert!(
            !annotation.annotation_type.is_empty(),
            "Annotation type should not be empty"
        );
    }
}

// ============================================================================
// qpdf / split_pdf tests
// ============================================================================

use pdf_mcp_server::pdf::QpdfWrapper;

/// Test splitting pages from a PDF
#[test]
fn test_split_pdf_basic() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Extract first 3 pages
    let result = QpdfWrapper::split_pages(&data, "1-3", None);
    assert!(result.is_ok(), "split_pages should succeed");

    let output_data = result.unwrap();
    assert!(!output_data.is_empty(), "Output PDF should not be empty");

    // Verify the output is a valid PDF
    let page_count = QpdfWrapper::get_page_count(&output_data, None);
    assert!(page_count.is_ok(), "Should be able to get page count");
    assert_eq!(page_count.unwrap(), 3, "Output should have 3 pages");
}

/// Test splitting single page
#[test]
fn test_split_pdf_single_page() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::split_pages(&data, "1", None);
    assert!(result.is_ok(), "split_pages should succeed for single page");

    let output_data = result.unwrap();
    let page_count = QpdfWrapper::get_page_count(&output_data, None).unwrap();
    assert_eq!(page_count, 1, "Output should have 1 page");
}

/// Test splitting with comma-separated pages
#[test]
fn test_split_pdf_comma_separated() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Extract pages 1, 3, 5
    let result = QpdfWrapper::split_pages(&data, "1,3,5", None);
    assert!(
        result.is_ok(),
        "split_pages should succeed for comma-separated pages"
    );

    let output_data = result.unwrap();
    let page_count = QpdfWrapper::get_page_count(&output_data, None).unwrap();
    assert_eq!(page_count, 3, "Output should have 3 pages");
}

/// Test splitting with reverse order
#[test]
fn test_split_pdf_reverse_order() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Get original page count
    let original_page_count = QpdfWrapper::get_page_count(&data, None).unwrap();

    // Extract all pages in reverse order
    let result = QpdfWrapper::split_pages(&data, "z-1", None);
    assert!(
        result.is_ok(),
        "split_pages should succeed for reverse order"
    );

    let output_data = result.unwrap();
    let page_count = QpdfWrapper::get_page_count(&output_data, None).unwrap();
    assert_eq!(
        page_count, original_page_count,
        "Reversed PDF should have same page count"
    );
}

/// Test splitting with odd pages
#[test]
fn test_split_pdf_odd_pages() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // qpdf requires a range before :odd/:even modifier (e.g., "1-z:odd")
    let result = QpdfWrapper::split_pages(&data, "1-z:odd", None);
    assert!(result.is_ok(), "split_pages should succeed for odd pages");

    let output_data = result.unwrap();
    let page_count = QpdfWrapper::get_page_count(&output_data, None).unwrap();
    assert!(page_count > 0, "Output should have at least 1 page");
}

/// Test splitting with even pages
#[test]
fn test_split_pdf_even_pages() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // qpdf requires a range before :odd/:even modifier (e.g., "1-z:even")
    let result = QpdfWrapper::split_pages(&data, "1-z:even", None);
    assert!(result.is_ok(), "split_pages should succeed for even pages");

    let output_data = result.unwrap();
    let page_count = QpdfWrapper::get_page_count(&output_data, None).unwrap();
    assert!(page_count > 0, "Output should have at least 1 page");
}

/// Test splitting with last page reference (r1)
#[test]
fn test_split_pdf_last_page() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::split_pages(&data, "r1", None);
    assert!(
        result.is_ok(),
        "split_pages should succeed for last page reference"
    );

    let output_data = result.unwrap();
    let page_count = QpdfWrapper::get_page_count(&output_data, None).unwrap();
    assert_eq!(page_count, 1, "Output should have 1 page (last page)");
}

/// Test splitting password-protected PDF with correct password
#[test]
fn test_split_pdf_password_protected() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::split_pages(&data, "1", Some("testpass"));
    assert!(
        result.is_ok(),
        "split_pages should succeed with correct password"
    );

    let output_data = result.unwrap();
    let page_count = QpdfWrapper::get_page_count(&output_data, None).unwrap();
    assert_eq!(page_count, 1, "Output should have 1 page");
}

/// Test splitting password-protected PDF without password fails
#[test]
fn test_split_pdf_password_required() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::split_pages(&data, "1", None);
    assert!(
        result.is_err(),
        "split_pages should fail without password for protected PDF"
    );
}

/// Test get_page_count function
#[test]
fn test_get_page_count() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::get_page_count(&data, None);
    assert!(result.is_ok(), "get_page_count should succeed");

    let page_count = result.unwrap();
    assert!(page_count > 0, "PDF should have at least 1 page");
}

/// Test get_page_count with password-protected PDF
#[test]
fn test_get_page_count_password_protected() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::get_page_count(&data, Some("testpass"));
    assert!(
        result.is_ok(),
        "get_page_count should succeed with correct password"
    );
}

/// Test get_page_count without password fails for protected PDF
#[test]
fn test_get_page_count_password_required() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::get_page_count(&data, None);
    assert!(
        result.is_err(),
        "get_page_count should fail without password for protected PDF"
    );
}

/// Test splitting with invalid PDF data fails
#[test]
fn test_split_pdf_invalid_data() {
    let result = QpdfWrapper::split_pages(b"not a valid PDF", "1", None);
    assert!(
        result.is_err(),
        "split_pages should fail for invalid PDF data"
    );
}

/// Test splitting with output file
#[test]
fn test_split_pdf_with_output_file() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Create a temporary directory for output
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let output_path = temp_dir.path().join("split_output.pdf");

    // Split pages and write to temp file
    let result = QpdfWrapper::split_pages(&data, "1-2", None);
    assert!(result.is_ok(), "split_pages should succeed");

    let output_data = result.unwrap();
    std::fs::write(&output_path, &output_data).expect("Failed to write output file");

    // Verify the output file exists and is a valid PDF
    assert!(output_path.exists(), "Output file should exist");
    let saved_data = std::fs::read(&output_path).expect("Failed to read output file");
    let page_count = QpdfWrapper::get_page_count(&saved_data, None).unwrap();
    assert_eq!(page_count, 2, "Saved PDF should have 2 pages");
}

// ============================================================================
// merge_pdfs tests
// ============================================================================

/// Test merging two PDFs
#[test]
fn test_merge_pdfs_basic() {
    let path1 = fixture_path("dummy.pdf");
    let path2 = fixture_path("tracemonkey.pdf");
    let data1 = std::fs::read(&path1).expect("Failed to read dummy.pdf");
    let data2 = std::fs::read(&path2).expect("Failed to read tracemonkey.pdf");

    let page_count1 = QpdfWrapper::get_page_count(&data1, None).unwrap();
    let page_count2 = QpdfWrapper::get_page_count(&data2, None).unwrap();

    let result = QpdfWrapper::merge(&[&data1, &data2]);
    assert!(result.is_ok(), "merge should succeed");

    let output_data = result.unwrap();
    let output_page_count = QpdfWrapper::get_page_count(&output_data, None).unwrap();
    assert_eq!(
        output_page_count,
        page_count1 + page_count2,
        "Merged PDF should have sum of page counts"
    );
}

/// Test merging single PDF (should still work)
#[test]
fn test_merge_pdfs_single() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let original_page_count = QpdfWrapper::get_page_count(&data, None).unwrap();

    let result = QpdfWrapper::merge(&[&data]);
    assert!(result.is_ok(), "merge with single PDF should succeed");

    let output_data = result.unwrap();
    let output_page_count = QpdfWrapper::get_page_count(&output_data, None).unwrap();
    assert_eq!(
        output_page_count, original_page_count,
        "Single merged PDF should have same page count"
    );
}

/// Test merging three PDFs
#[test]
fn test_merge_pdfs_multiple() {
    let path1 = fixture_path("dummy.pdf");
    let path2 = fixture_path("basicapi.pdf");
    let path3 = fixture_path("test-with-outline-and-images.pdf");
    let data1 = std::fs::read(&path1).expect("Failed to read dummy.pdf");
    let data2 = std::fs::read(&path2).expect("Failed to read basicapi.pdf");
    let data3 = std::fs::read(&path3).expect("Failed to read test-with-outline-and-images.pdf");

    let page_count1 = QpdfWrapper::get_page_count(&data1, None).unwrap();
    let page_count2 = QpdfWrapper::get_page_count(&data2, None).unwrap();
    let page_count3 = QpdfWrapper::get_page_count(&data3, None).unwrap();

    let result = QpdfWrapper::merge(&[&data1, &data2, &data3]);
    assert!(result.is_ok(), "merge should succeed for multiple PDFs");

    let output_data = result.unwrap();
    let output_page_count = QpdfWrapper::get_page_count(&output_data, None).unwrap();
    assert_eq!(
        output_page_count,
        page_count1 + page_count2 + page_count3,
        "Merged PDF should have sum of all page counts"
    );
}

/// Test merging with empty input fails
#[test]
fn test_merge_pdfs_empty_input() {
    let result = QpdfWrapper::merge(&[]);
    assert!(result.is_err(), "merge with no inputs should fail");
}

/// Test merging with invalid PDF data fails
#[test]
fn test_merge_pdfs_invalid_data() {
    let invalid_data = b"not a valid PDF";
    let result = QpdfWrapper::merge(&[invalid_data]);
    assert!(result.is_err(), "merge with invalid PDF should fail");
}

// ============================================================================
// protect_pdf / encrypt tests
// ============================================================================

/// Test encrypting a PDF
#[test]
fn test_encrypt_pdf_basic() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::encrypt(&data, "testpass", None, "full", true, true, None);
    assert!(result.is_ok(), "encrypt should succeed");

    let encrypted_data = result.unwrap();

    // Verify the encrypted PDF requires password
    let open_without_pass = QpdfWrapper::get_page_count(&encrypted_data, None);
    assert!(
        open_without_pass.is_err(),
        "Encrypted PDF should require password"
    );

    // Verify it works with correct password
    let open_with_pass = QpdfWrapper::get_page_count(&encrypted_data, Some("testpass"));
    assert!(
        open_with_pass.is_ok(),
        "Encrypted PDF should open with correct password"
    );
}

/// Test encrypting with separate user and owner passwords
#[test]
fn test_encrypt_pdf_dual_password() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::encrypt(
        &data,
        "userpass",
        Some("ownerpass"),
        "full",
        true,
        true,
        None,
    );
    assert!(result.is_ok(), "encrypt with dual passwords should succeed");

    let encrypted_data = result.unwrap();

    // Both passwords should work to open the PDF
    let open_with_user = QpdfWrapper::get_page_count(&encrypted_data, Some("userpass"));
    assert!(open_with_user.is_ok(), "Should open with user password");

    let open_with_owner = QpdfWrapper::get_page_count(&encrypted_data, Some("ownerpass"));
    assert!(open_with_owner.is_ok(), "Should open with owner password");
}

/// Test encrypting with print restriction
#[test]
fn test_encrypt_pdf_no_print() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::encrypt(&data, "testpass", None, "none", true, true, None);
    assert!(
        result.is_ok(),
        "encrypt with print restriction should succeed"
    );

    // Just verify the PDF was created successfully
    let encrypted_data = result.unwrap();
    let page_count = QpdfWrapper::get_page_count(&encrypted_data, Some("testpass"));
    assert!(page_count.is_ok(), "Should be able to read encrypted PDF");
}

/// Test encrypting with copy restriction
#[test]
fn test_encrypt_pdf_no_copy() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::encrypt(&data, "testpass", None, "full", false, true, None);
    assert!(
        result.is_ok(),
        "encrypt with copy restriction should succeed"
    );

    let encrypted_data = result.unwrap();
    let page_count = QpdfWrapper::get_page_count(&encrypted_data, Some("testpass"));
    assert!(page_count.is_ok(), "Should be able to read encrypted PDF");
}

/// Test encrypting with modify restriction
#[test]
fn test_encrypt_pdf_no_modify() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::encrypt(&data, "testpass", None, "full", true, false, None);
    assert!(
        result.is_ok(),
        "encrypt with modify restriction should succeed"
    );

    let encrypted_data = result.unwrap();
    let page_count = QpdfWrapper::get_page_count(&encrypted_data, Some("testpass"));
    assert!(page_count.is_ok(), "Should be able to read encrypted PDF");
}

/// Test encrypting with all restrictions
#[test]
fn test_encrypt_pdf_all_restrictions() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::encrypt(&data, "testpass", None, "none", false, false, None);
    assert!(
        result.is_ok(),
        "encrypt with all restrictions should succeed"
    );

    let encrypted_data = result.unwrap();
    let page_count = QpdfWrapper::get_page_count(&encrypted_data, Some("testpass"));
    assert!(page_count.is_ok(), "Should be able to read encrypted PDF");
}

/// Test encrypting already encrypted PDF with correct source password
#[test]
fn test_encrypt_already_encrypted_pdf() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::encrypt(
        &data,
        "newpass",
        None,
        "full",
        true,
        true,
        Some("testpass"), // source password
    );
    assert!(
        result.is_ok(),
        "encrypt already encrypted PDF should succeed with correct source password"
    );

    let encrypted_data = result.unwrap();

    // Old password should not work
    let open_with_old = QpdfWrapper::get_page_count(&encrypted_data, Some("testpass"));
    assert!(open_with_old.is_err(), "Old password should not work");

    // New password should work
    let open_with_new = QpdfWrapper::get_page_count(&encrypted_data, Some("newpass"));
    assert!(open_with_new.is_ok(), "New password should work");
}

/// Test encrypting already encrypted PDF without source password fails
#[test]
fn test_encrypt_already_encrypted_pdf_no_source_password() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::encrypt(&data, "newpass", None, "full", true, true, None);
    assert!(
        result.is_err(),
        "encrypt already encrypted PDF should fail without source password"
    );
}

// ============================================================================
// unprotect_pdf / decrypt tests
// ============================================================================

/// Test decrypting a PDF
#[test]
fn test_decrypt_pdf_basic() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::decrypt(&data, "testpass");
    assert!(
        result.is_ok(),
        "decrypt should succeed with correct password"
    );

    let decrypted_data = result.unwrap();

    // Verify the decrypted PDF does not require password
    let open_without_pass = QpdfWrapper::get_page_count(&decrypted_data, None);
    assert!(
        open_without_pass.is_ok(),
        "Decrypted PDF should not require password"
    );
}

/// Test decrypting with wrong password fails
#[test]
fn test_decrypt_pdf_wrong_password() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::decrypt(&data, "wrongpassword");
    assert!(result.is_err(), "decrypt should fail with wrong password");
}

/// Test decrypting non-encrypted PDF
#[test]
fn test_decrypt_non_encrypted_pdf() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // qpdf will succeed even on non-encrypted PDFs (it just passes through)
    let result = QpdfWrapper::decrypt(&data, "anypassword");
    // This might succeed or fail depending on qpdf behavior
    // Either way is acceptable
    let _ = result;
}

/// Test encrypt then decrypt round-trip
#[test]
fn test_encrypt_decrypt_roundtrip() {
    let path = fixture_path("dummy.pdf");
    let original_data = std::fs::read(&path).expect("Failed to read PDF file");
    let original_page_count = QpdfWrapper::get_page_count(&original_data, None).unwrap();

    // Encrypt
    let encrypted =
        QpdfWrapper::encrypt(&original_data, "mypassword", None, "full", true, true, None)
            .expect("encrypt should succeed");

    // Verify encryption
    assert!(
        QpdfWrapper::get_page_count(&encrypted, None).is_err(),
        "Encrypted PDF should require password"
    );

    // Decrypt
    let decrypted = QpdfWrapper::decrypt(&encrypted, "mypassword").expect("decrypt should succeed");

    // Verify decryption
    let decrypted_page_count = QpdfWrapper::get_page_count(&decrypted, None).unwrap();
    assert_eq!(
        decrypted_page_count, original_page_count,
        "Decrypted PDF should have same page count as original"
    );
}

/// Test chained operations: merge then encrypt
#[test]
fn test_merge_then_encrypt() {
    let path1 = fixture_path("dummy.pdf");
    let path2 = fixture_path("basicapi.pdf");
    let data1 = std::fs::read(&path1).expect("Failed to read dummy.pdf");
    let data2 = std::fs::read(&path2).expect("Failed to read basicapi.pdf");

    // Merge
    let merged = QpdfWrapper::merge(&[&data1, &data2]).expect("merge should succeed");
    let merged_page_count = QpdfWrapper::get_page_count(&merged, None).unwrap();

    // Encrypt
    let encrypted = QpdfWrapper::encrypt(&merged, "secret", None, "full", true, true, None)
        .expect("encrypt should succeed");

    // Verify
    assert!(
        QpdfWrapper::get_page_count(&encrypted, None).is_err(),
        "Encrypted merged PDF should require password"
    );
    let encrypted_page_count = QpdfWrapper::get_page_count(&encrypted, Some("secret")).unwrap();
    assert_eq!(
        encrypted_page_count, merged_page_count,
        "Encrypted PDF should have same page count as merged"
    );
}

/// Test chained operations: decrypt then split
#[test]
fn test_decrypt_then_split() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Decrypt
    let decrypted = QpdfWrapper::decrypt(&data, "testpass").expect("decrypt should succeed");

    // Split
    let split = QpdfWrapper::split_pages(&decrypted, "1", None).expect("split should succeed");

    // Verify
    let split_page_count = QpdfWrapper::get_page_count(&split, None).unwrap();
    assert_eq!(split_page_count, 1, "Split PDF should have 1 page");
}

// ============================================================================
// extract_links tests
// ============================================================================

/// Test extracting links from PDF
#[test]
fn test_extract_links_basic() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = extract_links(&data, None, None);
    assert!(result.is_ok(), "extract_links should succeed");

    // Links may or may not exist depending on the PDF
    let links = result.unwrap();
    let _ = links; // Just verify no panic
}

/// Test extracting links from specific pages
#[test]
fn test_extract_links_specific_pages() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = extract_links(&data, None, Some(&[1, 2]));
    assert!(
        result.is_ok(),
        "extract_links with page filter should succeed"
    );

    let links = result.unwrap();
    // All links should be from pages 1 or 2
    for link in &links {
        assert!(
            link.page == 1 || link.page == 2,
            "Links should only be from specified pages"
        );
    }
}

/// Test extracting links from password-protected PDF
#[test]
fn test_extract_links_password_protected() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = extract_links(&data, Some("testpass"), None);
    assert!(
        result.is_ok(),
        "extract_links should succeed with correct password"
    );
}

/// Test extracting links without password fails
#[test]
fn test_extract_links_password_required() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = extract_links(&data, None, None);
    assert!(
        result.is_err(),
        "extract_links should fail without password for protected PDF"
    );
}

// ============================================================================
// get_page_info tests
// ============================================================================

/// Test getting page info
#[test]
fn test_get_page_info_basic() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = get_page_info(&data, None);
    assert!(result.is_ok(), "get_page_info should succeed");

    let pages = result.unwrap();
    assert!(!pages.is_empty(), "Should have at least one page");

    // Check first page has valid dimensions
    let first = &pages[0];
    assert_eq!(first.page, 1, "First page should be page 1");
    assert!(first.width > 0.0, "Width should be positive");
    assert!(first.height > 0.0, "Height should be positive");
    assert!(
        first.rotation == 0
            || first.rotation == 90
            || first.rotation == 180
            || first.rotation == 270,
        "Rotation should be valid"
    );
    assert!(
        first.orientation == "portrait"
            || first.orientation == "landscape"
            || first.orientation == "square",
        "Orientation should be valid"
    );
}

/// Test page info includes token estimates and word count
#[test]
fn test_get_page_info_token_estimate() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = get_page_info(&data, None);
    assert!(result.is_ok(), "get_page_info should succeed");

    let pages = result.unwrap();

    // Find a page with content (tracemonkey should have text)
    let total_chars: usize = pages.iter().map(|p| p.char_count).sum();
    let total_words: usize = pages.iter().map(|p| p.word_count).sum();
    let total_tokens: usize = pages.iter().map(|p| p.estimated_token_count).sum();

    assert!(total_chars > 0, "Should have some characters");
    assert!(total_words > 0, "Should have some words");
    assert!(total_tokens > 0, "Should have estimated tokens");

    // Word count should be less than char count
    assert!(
        total_words < total_chars,
        "Word count should be less than char count"
    );

    // For English text, token count should generally be less than char count
    // (but for CJK it could be higher, so we just check it's reasonable)
    assert!(
        total_tokens > 0 && total_tokens < total_chars * 3,
        "Token count should be reasonable"
    );
}

/// Test get_page_info with password-protected PDF
#[test]
fn test_get_page_info_password_protected() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = get_page_info(&data, Some("testpass"));
    assert!(
        result.is_ok(),
        "get_page_info should succeed with correct password"
    );
}

/// Test get_page_info without password fails
#[test]
fn test_get_page_info_password_required() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = get_page_info(&data, None);
    assert!(
        result.is_err(),
        "get_page_info should fail without password for protected PDF"
    );
}

// ============================================================================
// compress_pdf tests
// ============================================================================

/// Test basic PDF compression
#[test]
fn test_compress_pdf_basic() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");
    let original_size = data.len();

    let result = QpdfWrapper::compress(&data, None, None, None);
    assert!(result.is_ok(), "compress should succeed");

    let compressed = result.unwrap();
    let compressed_size = compressed.len();

    // Compressed PDF should still be valid
    let page_count = QpdfWrapper::get_page_count(&compressed, None).unwrap();
    assert!(page_count > 0, "Compressed PDF should have pages");

    // Log compression ratio (not an assertion as compression may vary)
    let ratio = compressed_size as f64 / original_size as f64;
    println!(
        "Compression: {} -> {} bytes (ratio: {:.2})",
        original_size, compressed_size, ratio
    );
}

/// Test compression with different levels
#[test]
fn test_compress_pdf_levels() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Test with lowest compression
    let result_low = QpdfWrapper::compress(&data, None, Some("generate"), Some(1));
    assert!(result_low.is_ok(), "compress with level 1 should succeed");

    // Test with highest compression
    let result_high = QpdfWrapper::compress(&data, None, Some("generate"), Some(9));
    assert!(result_high.is_ok(), "compress with level 9 should succeed");

    // Both should produce valid PDFs
    let low_data = result_low.unwrap();
    let high_data = result_high.unwrap();

    assert!(
        QpdfWrapper::get_page_count(&low_data, None).is_ok(),
        "Low compression output should be valid"
    );
    assert!(
        QpdfWrapper::get_page_count(&high_data, None).is_ok(),
        "High compression output should be valid"
    );
}

/// Test compression with password-protected PDF
#[test]
fn test_compress_pdf_password_protected() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::compress(&data, Some("testpass"), None, None);
    assert!(
        result.is_ok(),
        "compress should succeed with correct password"
    );

    let compressed = result.unwrap();
    // Output should be decrypted
    let page_count = QpdfWrapper::get_page_count(&compressed, None);
    assert!(
        page_count.is_ok(),
        "Compressed output should be decrypted and accessible"
    );
}

/// Test compression without password fails for protected PDF
#[test]
fn test_compress_pdf_password_required() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = QpdfWrapper::compress(&data, None, None, None);
    assert!(
        result.is_err(),
        "compress should fail without password for protected PDF"
    );
}

// ============================================================================
// LLM-Optimized Text Extraction Tests
// ============================================================================

use pdf_mcp_server::pdf::{extract_text_with_options, TextExtractionConfig};

/// Test basic text extraction with LLM-optimized defaults
#[test]
fn test_extract_text_with_options_basic() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let config = TextExtractionConfig::default();

    let result = extract_text_with_options(&data, None, None, &config);
    assert!(result.is_ok(), "extract_text_with_options should succeed");

    let pages = result.unwrap();
    assert!(!pages.is_empty(), "Should extract at least one page");
}

/// Test extraction with dynamic thresholds (enabled by default)
#[test]
fn test_extract_text_with_dynamic_thresholds() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let config = TextExtractionConfig::default();

    let result = extract_text_with_options(&data, None, Some(&[1]), &config);
    assert!(
        result.is_ok(),
        "extract_text_with_options with dynamic thresholds should succeed"
    );

    let pages = result.unwrap();
    assert_eq!(pages.len(), 1, "Should extract exactly one page");
    assert!(!pages[0].1.is_empty(), "Extracted text should not be empty");
}

/// Test extraction with paragraph detection (enabled by default)
#[test]
fn test_extract_text_with_paragraph_detection() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Default config has paragraph_mode: "spacing"
    let config = TextExtractionConfig::default();

    let result = extract_text_with_options(&data, None, Some(&[1]), &config);
    assert!(
        result.is_ok(),
        "extract_text_with_options with paragraph detection should succeed"
    );

    let pages = result.unwrap();
    assert!(!pages[0].1.is_empty(), "Extracted text should not be empty");
}

/// Test extraction with column detection (tracemonkey.pdf is 2-column, enabled by default)
#[test]
fn test_extract_text_with_column_detection() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Default config has column_mode: "auto"
    let config = TextExtractionConfig::default();

    let result = extract_text_with_options(&data, None, Some(&[1]), &config);
    assert!(
        result.is_ok(),
        "extract_text_with_options with column detection should succeed"
    );

    let pages = result.unwrap();
    assert!(!pages[0].1.is_empty(), "Extracted text should not be empty");

    // The text should be extracted (we can't easily verify order without manual inspection)
    println!("Column detection extracted {} chars", pages[0].1.len());
}

/// Test extraction with all LLM options (all enabled by default)
#[test]
fn test_extract_text_with_all_options() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    // Default config has all LLM optimizations enabled
    let config = TextExtractionConfig::default();

    let result = extract_text_with_options(&data, None, Some(&[1, 2]), &config);
    assert!(
        result.is_ok(),
        "extract_text_with_options with all options should succeed"
    );

    let pages = result.unwrap();
    assert_eq!(pages.len(), 2, "Should extract exactly 2 pages");
    assert!(!pages[0].1.is_empty(), "Page 1 text should not be empty");
    assert!(!pages[1].1.is_empty(), "Page 2 text should not be empty");
}

/// Test extraction with password-protected PDF
#[test]
fn test_extract_text_with_options_password_protected() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let config = TextExtractionConfig::default();

    // Should work with correct password
    let result = extract_text_with_options(&data, Some("testpass"), None, &config);
    assert!(
        result.is_ok(),
        "extract_text_with_options should succeed with correct password"
    );

    // Should fail without password
    let result_no_pass = extract_text_with_options(&data, None, None, &config);
    assert!(
        result_no_pass.is_err(),
        "extract_text_with_options should fail without password"
    );
}

/// Test extraction with specific page range
#[test]
fn test_extract_text_with_options_page_range() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let config = TextExtractionConfig::default();

    // Extract only pages 3-5
    let result = extract_text_with_options(&data, None, Some(&[3, 4, 5]), &config);
    assert!(result.is_ok(), "extract_text_with_options should succeed");

    let pages = result.unwrap();
    assert_eq!(pages.len(), 3, "Should extract exactly 3 pages");
    assert_eq!(pages[0].0, 3, "First page should be page 3");
    assert_eq!(pages[1].0, 4, "Second page should be page 4");
    assert_eq!(pages[2].0, 5, "Third page should be page 5");
}

/// Test extraction with invalid PDF data
#[test]
fn test_extract_text_with_options_invalid_pdf() {
    let config = TextExtractionConfig::default();

    let result = extract_text_with_options(b"not a valid PDF", None, None, &config);
    assert!(
        result.is_err(),
        "extract_text_with_options should fail for invalid PDF"
    );
}

// ============================================================================
// list_pdfs tests
// ============================================================================

use pdf_mcp_server::server::{ListPdfsParams, PdfServer};

/// Test listing PDFs in the fixtures directory
#[test]
fn test_list_pdfs_basic() {
    let fixtures_dir = fixture_path("");
    let params = ListPdfsParams {
        directory: fixtures_dir.to_string_lossy().to_string(),
        recursive: false,
        pattern: None,
    };

    let result = PdfServer::process_list_pdfs_public(&params);
    assert!(result.is_ok(), "list_pdfs should succeed");

    let list_result = result.unwrap();
    assert!(list_result.error.is_none(), "Should not have error");
    assert!(list_result.total_count > 0, "Should find PDF files");

    // Verify all returned files are PDFs
    for file in &list_result.files {
        assert!(
            file.name.to_lowercase().ends_with(".pdf"),
            "All files should be PDFs"
        );
        assert!(file.size > 0, "File size should be positive");
    }
}

/// Test listing PDFs with pattern filter
#[test]
fn test_list_pdfs_with_pattern() {
    let fixtures_dir = fixture_path("");
    let params = ListPdfsParams {
        directory: fixtures_dir.to_string_lossy().to_string(),
        recursive: false,
        pattern: Some("dummy*".to_string()),
    };

    let result = PdfServer::process_list_pdfs_public(&params);
    assert!(result.is_ok(), "list_pdfs with pattern should succeed");

    let list_result = result.unwrap();
    // All returned files should match the pattern
    for file in &list_result.files {
        assert!(
            file.name.starts_with("dummy"),
            "All files should match pattern"
        );
    }
}

/// Test listing PDFs in non-existent directory
#[test]
fn test_list_pdfs_nonexistent_directory() {
    let params = ListPdfsParams {
        directory: "/nonexistent/directory/path".to_string(),
        recursive: false,
        pattern: None,
    };

    let result = PdfServer::process_list_pdfs_public(&params);
    assert!(
        result.is_err(),
        "list_pdfs should fail for non-existent directory"
    );
}

/// Test listing PDFs with file path instead of directory
#[test]
fn test_list_pdfs_not_a_directory() {
    let file_path = fixture_path("dummy.pdf");
    let params = ListPdfsParams {
        directory: file_path.to_string_lossy().to_string(),
        recursive: false,
        pattern: None,
    };

    let result = PdfServer::process_list_pdfs_public(&params);
    assert!(result.is_err(), "list_pdfs should fail for file path");
}

// ============================================================================
// PdfServer resource directory tests
// ============================================================================

/// Test PdfServer creation with resource directories
#[test]
fn test_pdf_server_with_resource_dirs() {
    let fixtures_dir = fixture_path("");
    let server = PdfServer::with_resource_dirs(vec![fixtures_dir.to_string_lossy().to_string()]);

    // Server should be created successfully
    // The actual resource listing is tested via async methods
    drop(server);
}

/// Test PdfServer creation without resource directories
#[test]
fn test_pdf_server_without_resource_dirs() {
    let server = PdfServer::new();

    // Server should be created successfully without resource dirs
    drop(server);
}

// ============================================================================
// convert_page_to_image / render_pages_to_images tests
// ============================================================================

/// Test rendering a single page to image
#[test]
fn test_render_page_to_image_single() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = render_pages_to_images(&data, None, &[1], None, None, None);
    assert!(result.is_ok(), "render_pages_to_images should succeed");

    let rendered = result.unwrap();
    assert_eq!(rendered.len(), 1, "Should render exactly 1 page");

    let page = &rendered[0];
    assert_eq!(page.page, 1);
    assert!(page.width > 0);
    assert!(page.height > 0);
    assert!(!page.data_base64.is_empty());
    assert_eq!(page.mime_type, "image/png");

    // Verify PNG header in base64-decoded data
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&page.data_base64)
        .expect("Should be valid base64");
    assert!(decoded.len() >= 8);
    assert_eq!(
        &decoded[0..8],
        &[137, 80, 78, 71, 13, 10, 26, 10],
        "Should have PNG header"
    );
}

/// Test rendering multiple pages
#[test]
fn test_render_pages_to_images_multiple() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = render_pages_to_images(&data, None, &[1, 2, 3], None, None, None);
    assert!(result.is_ok(), "render_pages_to_images should succeed");

    let rendered = result.unwrap();
    assert_eq!(rendered.len(), 3, "Should render 3 pages");
    assert_eq!(rendered[0].page, 1);
    assert_eq!(rendered[1].page, 2);
    assert_eq!(rendered[2].page, 3);
}

/// Test rendering with custom width
#[test]
fn test_render_page_with_custom_width() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = render_pages_to_images(&data, None, &[1], Some(800), None, None);
    assert!(result.is_ok(), "render with custom width should succeed");

    let rendered = result.unwrap();
    assert_eq!(rendered.len(), 1);
    // Width should be approximately 800 (may not be exact due to aspect ratio)
    assert!(rendered[0].width > 0);
}

/// Test rendering with scale factor
#[test]
fn test_render_page_with_scale() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = render_pages_to_images(&data, None, &[1], None, None, Some(2.0));
    assert!(result.is_ok(), "render with scale should succeed");

    let rendered = result.unwrap();
    assert_eq!(rendered.len(), 1);
    assert!(rendered[0].width > 0);
}

/// Test rendering password-protected PDF
#[test]
fn test_render_page_password_protected() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = render_pages_to_images(&data, Some("testpass"), &[1], None, None, None);
    assert!(
        result.is_ok(),
        "render password-protected PDF should succeed"
    );

    let rendered = result.unwrap();
    assert_eq!(rendered.len(), 1);
}

/// Test rendering without password fails
#[test]
fn test_render_page_password_required() {
    let path = fixture_path("password-protected.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = render_pages_to_images(&data, None, &[1], None, None, None);
    assert!(result.is_err(), "Should fail without password");
}

/// Test rendering with invalid page numbers (should skip)
#[test]
fn test_render_page_invalid_page() {
    let path = fixture_path("dummy.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = render_pages_to_images(&data, None, &[9999], None, None, None);
    assert!(result.is_ok(), "Should succeed but return empty");

    let rendered = result.unwrap();
    assert!(rendered.is_empty(), "Invalid page should be skipped");
}

/// Test rendering invalid PDF fails
#[test]
fn test_render_page_invalid_pdf() {
    let result = render_pages_to_images(b"not a valid PDF", None, &[1], None, None, None);
    assert!(result.is_err(), "Should fail for invalid PDF");
}

// ============================================================================
// extract_form_fields tests
// ============================================================================

/// Test extracting form fields from a form PDF
#[test]
fn test_extract_form_fields_basic() {
    let path = fixture_path("form-test.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = extract_form_fields(&data, None, None);
    assert!(result.is_ok(), "extract_form_fields should succeed");

    let fields = result.unwrap();
    assert!(!fields.is_empty(), "Form PDF should have fields");

    // Check that we found expected field types
    let field_types: Vec<&str> = fields.iter().map(|f| f.field_type.as_str()).collect();
    assert!(
        field_types.contains(&"text") || field_types.contains(&"checkbox"),
        "Should find text or checkbox fields"
    );
}

/// Test extracting form fields from non-form PDF
#[test]
fn test_extract_form_fields_no_form() {
    let path = fixture_path("tracemonkey.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = extract_form_fields(&data, None, None);
    assert!(result.is_ok(), "Should succeed for non-form PDF");

    let fields = result.unwrap();
    // tracemonkey.pdf has no form fields, but may have widget annotations
    let _ = fields;
}

/// Test extracting form fields with page filter
#[test]
fn test_extract_form_fields_page_filter() {
    let path = fixture_path("form-test.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let result = extract_form_fields(&data, None, Some(&[1]));
    assert!(result.is_ok(), "Should succeed with page filter");

    let fields = result.unwrap();
    for field in &fields {
        assert_eq!(field.page, 1, "All fields should be from page 1");
    }
}

/// Test form field names are populated
#[test]
fn test_extract_form_fields_names() {
    let path = fixture_path("form-test.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let fields = extract_form_fields(&data, None, None).expect("Should succeed");

    // At least some fields should have names
    let named_fields: Vec<_> = fields.iter().filter(|f| f.name.is_some()).collect();
    assert!(!named_fields.is_empty(), "Should have fields with names");
}

/// Test extracting form fields from invalid PDF
#[test]
fn test_extract_form_fields_invalid_pdf() {
    let result = extract_form_fields(b"not a valid PDF", None, None);
    assert!(result.is_err(), "Should fail for invalid PDF");
}

// ============================================================================
// fill_form tests
// ============================================================================

/// Test filling a text field
#[test]
fn test_fill_form_text_field() {
    let path = fixture_path("form-test.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let values = vec![FormFieldValue {
        name: "full_name".to_string(),
        value: Some("Jane Smith".to_string()),
        checked: None,
    }];

    let result = fill_form_fields(&data, None, &values);
    assert!(result.is_ok(), "fill_form_fields should succeed");

    let (output_data, fill_result) = result.unwrap();
    assert!(!output_data.is_empty(), "Output PDF should not be empty");
    assert!(
        fill_result.fields_filled > 0 || !fill_result.fields_skipped.is_empty(),
        "Should have processed at least one field"
    );
}

/// Test filling nonexistent field (should be skipped)
#[test]
fn test_fill_form_nonexistent_field() {
    let path = fixture_path("form-test.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let values = vec![FormFieldValue {
        name: "nonexistent_field".to_string(),
        value: Some("test".to_string()),
        checked: None,
    }];

    let result = fill_form_fields(&data, None, &values);
    assert!(result.is_ok(), "Should succeed even with nonexistent field");

    let (_, fill_result) = result.unwrap();
    assert_eq!(fill_result.fields_filled, 0, "No fields should be filled");
    assert_eq!(
        fill_result.fields_skipped.len(),
        1,
        "Nonexistent field should be reported as skipped"
    );
    assert_eq!(fill_result.fields_skipped[0].name, "nonexistent_field");
}

/// Test fill_form produces valid PDF output
#[test]
fn test_fill_form_valid_output() {
    let path = fixture_path("form-test.pdf");
    let data = std::fs::read(&path).expect("Failed to read PDF file");

    let values = vec![FormFieldValue {
        name: "email".to_string(),
        value: Some("test@example.com".to_string()),
        checked: None,
    }];

    let result = fill_form_fields(&data, None, &values);
    assert!(result.is_ok(), "fill_form_fields should succeed");

    let (output_data, _) = result.unwrap();

    // Verify output is valid PDF (has PDF header)
    assert!(output_data.len() >= 4);
    assert_eq!(&output_data[0..4], b"%PDF", "Output should be a valid PDF");

    // Verify output can be opened
    let reader = PdfReader::open_bytes(&output_data, None);
    assert!(reader.is_ok(), "Output PDF should be readable");
}

/// Test fill_form with invalid PDF
#[test]
fn test_fill_form_invalid_pdf() {
    let values = vec![FormFieldValue {
        name: "test".to_string(),
        value: Some("value".to_string()),
        checked: None,
    }];

    let result = fill_form_fields(b"not a valid PDF", None, &values);
    assert!(result.is_err(), "Should fail for invalid PDF");
}

// ============================================================================
// summarize_structure tests (via server)
// ============================================================================

/// Test summarize_structure with multi-page PDF
#[tokio::test]
async fn test_summarize_structure_multi_page() {
    let server = PdfServer::new();
    let source = pdf_mcp_server::server::PdfSource::Path {
        path: fixture_path("tracemonkey.pdf")
            .to_string_lossy()
            .to_string(),
    };
    let params = pdf_mcp_server::server::SummarizeStructureParams {
        sources: vec![source.clone()],
        password: None,
        cache: false,
    };

    let result = server
        .process_summarize_structure(&source, &params)
        .await
        .unwrap();
    assert!(result.error.is_none());
    assert!(result.page_count > 0);
    assert!(result.file_size > 0);
    assert!(result.total_chars > 0);
    assert!(result.total_words > 0);
    assert!(result.total_estimated_tokens > 0);
    assert_eq!(result.pages.len(), result.page_count as usize);
    assert!(result.metadata.is_some());
}

/// Test summarize_structure with outline PDF
#[tokio::test]
async fn test_summarize_structure_with_outline() {
    let server = PdfServer::new();
    let source = pdf_mcp_server::server::PdfSource::Path {
        path: fixture_path("test-with-outline-and-images.pdf")
            .to_string_lossy()
            .to_string(),
    };
    let params = pdf_mcp_server::server::SummarizeStructureParams {
        sources: vec![source.clone()],
        password: None,
        cache: false,
    };

    let result = server
        .process_summarize_structure(&source, &params)
        .await
        .unwrap();
    assert!(result.error.is_none());
    assert!(result.page_count > 0);
}

/// Test summarize_structure with form PDF
#[tokio::test]
async fn test_summarize_structure_with_form() {
    let server = PdfServer::new();
    let source = pdf_mcp_server::server::PdfSource::Path {
        path: fixture_path("form-test.pdf").to_string_lossy().to_string(),
    };
    let params = pdf_mcp_server::server::SummarizeStructureParams {
        sources: vec![source.clone()],
        password: None,
        cache: false,
    };

    let result = server
        .process_summarize_structure(&source, &params)
        .await
        .unwrap();
    assert!(result.error.is_none());
    assert!(result.page_count > 0);
    assert!(result.has_form, "Form PDF should report has_form=true");
    assert!(result.form_field_count > 0, "Should have form fields");
    assert!(
        !result.form_field_types.is_empty(),
        "Should report form field types"
    );
}

/// Test summarize_structure with simple PDF
#[tokio::test]
async fn test_summarize_structure_simple() {
    let server = PdfServer::new();
    let source = pdf_mcp_server::server::PdfSource::Path {
        path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
    };
    let params = pdf_mcp_server::server::SummarizeStructureParams {
        sources: vec![source.clone()],
        password: None,
        cache: false,
    };

    let result = server
        .process_summarize_structure(&source, &params)
        .await
        .unwrap();
    assert!(result.error.is_none());
    assert!(result.page_count > 0);
    assert!(!result.is_encrypted);
}

// ============================================================================
// Server-level convert_page_to_image tests
// ============================================================================

/// Test server-level convert_page_to_image
#[tokio::test]
async fn test_server_convert_page_to_image() {
    let server = PdfServer::new();
    let source = pdf_mcp_server::server::PdfSource::Path {
        path: fixture_path("dummy.pdf").to_string_lossy().to_string(),
    };
    let params = pdf_mcp_server::server::ConvertPageToImageParams {
        sources: vec![source.clone()],
        pages: Some("1".to_string()),
        width: None,
        height: None,
        scale: None,
        password: None,
        cache: false,
    };

    let result = server
        .process_convert_page_to_image(&source, &params)
        .await
        .unwrap();
    assert!(result.error.is_none());
    assert_eq!(result.pages.len(), 1);
    assert!(!result.pages[0].data_base64.is_empty());
}

// ============================================================================
// Server-level extract_form_fields tests
// ============================================================================

/// Test server-level extract_form_fields
#[tokio::test]
async fn test_server_extract_form_fields() {
    let server = PdfServer::new();
    let source = pdf_mcp_server::server::PdfSource::Path {
        path: fixture_path("form-test.pdf").to_string_lossy().to_string(),
    };
    let params = pdf_mcp_server::server::ExtractFormFieldsParams {
        sources: vec![source.clone()],
        pages: None,
        password: None,
        cache: false,
    };

    let result = server
        .process_extract_form_fields(&source, &params)
        .await
        .unwrap();
    assert!(result.error.is_none());
    assert!(result.total_fields > 0);
}

// ============================================================================
// Server-level fill_form tests
// ============================================================================

/// Test server-level fill_form
#[tokio::test]
async fn test_server_fill_form() {
    let server = PdfServer::new();
    let params = pdf_mcp_server::server::FillFormParams {
        source: pdf_mcp_server::server::PdfSource::Path {
            path: fixture_path("form-test.pdf").to_string_lossy().to_string(),
        },
        field_values: vec![pdf_mcp_server::server::FormFieldValueParam {
            name: "full_name".to_string(),
            value: Some("Test User".to_string()),
            checked: None,
        }],
        output_path: None,
        password: None,
    };

    let result = server.process_fill_form(&params).await.unwrap();
    assert!(result.error.is_none());
    assert!(!result.output_cache_key.is_empty());
}
