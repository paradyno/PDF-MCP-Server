//! PDF reader wrapper for PDFium

use crate::error::{Error, Result};
use base64::Engine;
use pdfium_render::prelude::*;
use std::path::Path;

/// Get PDFium instance (creates new instance each time - PDFium is not thread-safe)
fn create_pdfium() -> Result<Pdfium> {
    // Try to bind to system library or use static linking
    let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
        .or_else(|_| {
            Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(
                "/opt/pdfium/lib",
            ))
        })
        .or_else(|_| Pdfium::bind_to_system_library())
        .map_err(|e| Error::Pdfium {
            reason: format!("Failed to initialize PDFium: {}", e),
        })?;

    Ok(Pdfium::new(bindings))
}

/// PDF metadata
#[derive(Debug, Clone, Default)]
pub struct PdfMetadataInfo {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub creation_date: Option<String>,
    pub modification_date: Option<String>,
}

/// Outline entry (bookmark)
#[derive(Debug, Clone)]
pub struct OutlineItem {
    pub title: String,
    pub page: Option<u32>,
    pub children: Vec<OutlineItem>,
}

/// Extracted image information
#[derive(Debug, Clone)]
pub struct ExtractedImage {
    /// Page number (1-indexed)
    pub page: u32,
    /// Image index on the page
    pub index: u32,
    /// Image width in pixels
    pub width: u32,
    /// Image height in pixels
    pub height: u32,
    /// Base64 encoded image data (PNG format)
    pub data_base64: String,
    /// MIME type
    pub mime_type: String,
}

/// Extracted link information
#[derive(Debug, Clone)]
pub struct PdfLink {
    /// Page number (1-indexed)
    pub page: u32,
    /// Link URL (for URI actions)
    pub url: Option<String>,
    /// Destination page (for internal links)
    pub dest_page: Option<u32>,
    /// Bounding rectangle (left, top, right, bottom)
    pub bounds: Option<(f32, f32, f32, f32)>,
    /// Text near or under the link area (if extractable)
    pub text: Option<String>,
}

/// Page dimension and content information
#[derive(Debug, Clone)]
pub struct PdfPageInfo {
    /// Page number (1-indexed)
    pub page: u32,
    /// Page width in points (1 point = 1/72 inch)
    pub width: f32,
    /// Page height in points
    pub height: f32,
    /// Page rotation in degrees (0, 90, 180, 270)
    pub rotation: i32,
    /// Page orientation based on dimensions
    pub orientation: String,
    /// Text content character count
    pub char_count: usize,
    /// Word count (whitespace-separated)
    pub word_count: usize,
    /// Estimated token count for LLMs.
    ///
    /// NOTE: This is an approximation. Actual token counts vary by model:
    /// - Latin/English: ~4 characters per token
    /// - CJK (Chinese/Japanese/Korean): ~2 tokens per character
    ///
    /// Based on Claude/GPT tokenizer analysis. Use as rough guidance only.
    pub estimated_token_count: usize,
}

/// Extracted annotation information
#[derive(Debug, Clone)]
pub struct PdfAnnotation {
    /// Page number (1-indexed)
    pub page: u32,
    /// Annotation type (e.g., "highlight", "text", "underline")
    pub annotation_type: String,
    /// Text content (comment, note)
    pub contents: Option<String>,
    /// Author name
    pub author: Option<String>,
    /// Creation date
    pub created: Option<String>,
    /// Modification date
    pub modified: Option<String>,
    /// Bounding rectangle (left, top, right, bottom)
    pub bounds: Option<(f32, f32, f32, f32)>,
    /// Highlighted/underlined text (extracted from page)
    pub highlighted_text: Option<String>,
    /// Fill color (hex format)
    pub color: Option<String>,
}

/// PDF reader using PDFium
pub struct PdfReader {
    // Store data to extend lifetime
    _data: Vec<u8>,
    page_count: u32,
    metadata: PdfMetadataInfo,
    outline: Vec<OutlineItem>,
    page_texts: Vec<String>,
}

impl PdfReader {
    /// Open a PDF from a file path
    pub fn open<P: AsRef<Path>>(path: P, password: Option<&str>) -> Result<Self> {
        let path = path.as_ref();

        if !path.exists() {
            return Err(Error::PdfNotFound {
                path: path.display().to_string(),
            });
        }

        let data = std::fs::read(path)?;
        Self::open_bytes(&data, password)
    }

    /// Open a PDF from bytes
    pub fn open_bytes(data: &[u8], password: Option<&str>) -> Result<Self> {
        if data.len() < 4 || &data[0..4] != b"%PDF" {
            return Err(Error::InvalidPdf {
                reason: "Not a valid PDF file".to_string(),
            });
        }

        let pdfium = create_pdfium()?;

        let document = match password {
            Some(pwd) => pdfium.load_pdf_from_byte_slice(data, Some(pwd)),
            None => pdfium.load_pdf_from_byte_slice(data, None),
        }
        .map_err(Self::map_pdfium_error)?;

        // Extract all data upfront
        let page_count = document.pages().len() as u32;
        let metadata = Self::extract_metadata(&document);
        let outline = Self::extract_outline(&document);
        let page_texts = Self::extract_all_page_texts(&document)?;

        Ok(Self {
            _data: data.to_vec(),
            page_count,
            metadata,
            outline,
            page_texts,
        })
    }

    /// Open a PDF from bytes, extracting only metadata (no text extraction for performance)
    pub fn open_bytes_metadata_only(data: &[u8], password: Option<&str>) -> Result<Self> {
        if data.len() < 4 || &data[0..4] != b"%PDF" {
            return Err(Error::InvalidPdf {
                reason: "Not a valid PDF file".to_string(),
            });
        }

        let pdfium = create_pdfium()?;

        let document = match password {
            Some(pwd) => pdfium.load_pdf_from_byte_slice(data, Some(pwd)),
            None => pdfium.load_pdf_from_byte_slice(data, None),
        }
        .map_err(Self::map_pdfium_error)?;

        // Extract only metadata and page count (skip text extraction)
        let page_count = document.pages().len() as u32;
        let metadata = Self::extract_metadata(&document);

        Ok(Self {
            _data: data.to_vec(),
            page_count,
            metadata,
            outline: Vec::new(),    // Skip outline extraction
            page_texts: Vec::new(), // Skip text extraction
        })
    }

    fn extract_metadata(document: &PdfDocument) -> PdfMetadataInfo {
        let meta = document.metadata();
        PdfMetadataInfo {
            title: meta
                .get(PdfDocumentMetadataTagType::Title)
                .map(|t| t.value().to_string()),
            author: meta
                .get(PdfDocumentMetadataTagType::Author)
                .map(|t| t.value().to_string()),
            subject: meta
                .get(PdfDocumentMetadataTagType::Subject)
                .map(|t| t.value().to_string()),
            creator: meta
                .get(PdfDocumentMetadataTagType::Creator)
                .map(|t| t.value().to_string()),
            producer: meta
                .get(PdfDocumentMetadataTagType::Producer)
                .map(|t| t.value().to_string()),
            creation_date: meta
                .get(PdfDocumentMetadataTagType::CreationDate)
                .map(|t| t.value().to_string()),
            modification_date: meta
                .get(PdfDocumentMetadataTagType::ModificationDate)
                .map(|t| t.value().to_string()),
        }
    }

    fn extract_outline(document: &PdfDocument) -> Vec<OutlineItem> {
        Self::collect_bookmarks(document.bookmarks().iter())
    }

    fn collect_bookmarks<'a>(bookmarks: impl Iterator<Item = PdfBookmark<'a>>) -> Vec<OutlineItem> {
        bookmarks
            .map(|bookmark| {
                let title = bookmark.title().unwrap_or_default();
                let page = bookmark.destination().and_then(|dest| {
                    dest.page_index().ok().map(|idx| idx as u32 + 1) // Convert to 1-indexed
                });

                let children = Self::collect_bookmarks(bookmark.iter_direct_children());

                OutlineItem {
                    title,
                    page,
                    children,
                }
            })
            .collect()
    }

    fn extract_all_page_texts(document: &PdfDocument) -> Result<Vec<String>> {
        let pages = document.pages();
        let page_len = pages.len() as usize;
        let mut texts = Vec::with_capacity(page_len);

        for index in 0..pages.len() {
            let page = pages.get(index).map_err(|e| Error::Pdfium {
                reason: format!("Failed to get page {}: {}", index + 1, e),
            })?;

            let text = Self::extract_page_text_with_layout(&page)?;
            texts.push(text);
        }

        Ok(texts)
    }

    /// Extract text from a page with Y-coordinate based ordering (preserves reading order)
    fn extract_page_text_with_layout(page: &PdfPage) -> Result<String> {
        let text_obj = match page.text() {
            Ok(t) => t,
            Err(_) => return Ok(String::new()),
        };

        // Collect all characters with their positions
        let mut chars_with_pos: Vec<(char, f32, f32)> = Vec::new();

        for segment in text_obj.segments().iter() {
            if let Ok(chars) = segment.chars() {
                for char_result in chars.iter() {
                    if let Some(c) = char_result.unicode_char() {
                        // Get character position using loose_bounds
                        if let Ok(bounds) = char_result.loose_bounds() {
                            let x = bounds.left().value;
                            let y = bounds.top().value; // Use top for Y coordinate
                            chars_with_pos.push((c, x, y));
                        }
                    }
                }
            }
        }

        if chars_with_pos.is_empty() {
            return Ok(String::new());
        }

        // Group characters by Y-coordinate (with tolerance for same-line detection)
        // Tolerance of ~5 points accounts for slight vertical variations within a line
        const Y_TOLERANCE: f32 = 5.0;

        // Sort by Y descending (top to bottom in PDF coordinates), then X ascending
        chars_with_pos.sort_by(|a, b| {
            let y_cmp = b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal);
            if y_cmp == std::cmp::Ordering::Equal {
                a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
            } else {
                y_cmp
            }
        });

        // Group into lines based on Y-coordinate proximity
        let mut lines: Vec<Vec<(char, f32)>> = Vec::new();
        let mut current_line: Vec<(char, f32)> = Vec::new();
        let mut current_y: Option<f32> = None;

        for (c, x, y) in chars_with_pos {
            match current_y {
                Some(cur_y) if (cur_y - y).abs() <= Y_TOLERANCE => {
                    // Same line
                    current_line.push((c, x));
                }
                _ => {
                    // New line
                    if !current_line.is_empty() {
                        lines.push(current_line);
                    }
                    current_line = vec![(c, x)];
                    current_y = Some(y);
                }
            }
        }

        // Don't forget the last line
        if !current_line.is_empty() {
            lines.push(current_line);
        }

        // Sort each line by X coordinate (left to right) and build the text
        let mut result = String::new();
        for mut line in lines {
            line.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            // Add space between characters if there's a gap (word separation)
            let mut prev_x: Option<f32> = None;
            const SPACE_THRESHOLD: f32 = 10.0; // Adjust based on typical character width

            for (c, x) in line {
                if let Some(px) = prev_x {
                    // If there's a significant gap, add a space
                    if x - px > SPACE_THRESHOLD && c != ' ' {
                        result.push(' ');
                    }
                }
                result.push(c);
                prev_x = Some(x);
            }

            result.push('\n');
        }

        Ok(result.trim_end().to_string())
    }

    /// Map PDFium errors to our error type
    fn map_pdfium_error(err: PdfiumError) -> Error {
        match err {
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                Error::PasswordRequired
            }
            _ => Error::Pdfium {
                reason: format!("{}", err),
            },
        }
    }

    /// Get the number of pages
    pub fn page_count(&self) -> u32 {
        self.page_count
    }

    /// Get PDF metadata
    pub fn metadata(&self) -> &PdfMetadataInfo {
        &self.metadata
    }

    /// Extract text from a specific page (1-indexed)
    pub fn extract_page_text(&self, page_num: u32) -> Result<String> {
        if page_num < 1 || page_num > self.page_count {
            return Err(Error::PageOutOfBounds {
                page: page_num,
                total: self.page_count,
            });
        }

        Ok(self.page_texts[(page_num - 1) as usize].clone())
    }

    /// Extract text from all pages
    pub fn extract_all_text(&self) -> Result<Vec<(u32, String)>> {
        let mut results = Vec::new();
        for page_num in 1..=self.page_count {
            let text = self.extract_page_text(page_num)?;
            results.push((page_num, text));
        }
        Ok(results)
    }

    /// Extract text from specified pages
    pub fn extract_pages_text(&self, pages: &[u32]) -> Result<Vec<(u32, String)>> {
        let mut results = Vec::new();
        for &page_num in pages {
            let text = self.extract_page_text(page_num)?;
            results.push((page_num, text));
        }
        Ok(results)
    }

    /// Get bookmarks/outline
    pub fn get_outline(&self) -> Vec<OutlineItem> {
        self.outline.clone()
    }

    /// Search for text in the document
    pub fn search(&self, query: &str, case_sensitive: bool) -> Vec<(u32, String)> {
        let mut matches = Vec::new();
        let search_query = if case_sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };

        for page_num in 1..=self.page_count {
            if let Ok(text) = self.extract_page_text(page_num) {
                let search_text = if case_sensitive {
                    text.clone()
                } else {
                    text.to_lowercase()
                };

                if search_text.contains(&search_query) {
                    matches.push((page_num, text));
                }
            }
        }
        matches
    }
}

/// Extract images from PDF bytes
/// This is a separate function because image extraction requires re-loading the document
pub fn extract_images(data: &[u8], password: Option<&str>) -> Result<Vec<ExtractedImage>> {
    if data.len() < 4 || &data[0..4] != b"%PDF" {
        return Err(Error::InvalidPdf {
            reason: "Not a valid PDF file".to_string(),
        });
    }

    let pdfium = create_pdfium()?;

    let document = match password {
        Some(pwd) => pdfium.load_pdf_from_byte_slice(data, Some(pwd)),
        None => pdfium.load_pdf_from_byte_slice(data, None),
    }
    .map_err(|e| match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            Error::PasswordRequired
        }
        _ => Error::Pdfium {
            reason: format!("{}", e),
        },
    })?;

    let mut images = Vec::new();
    let pages = document.pages();

    for page_index in 0..pages.len() {
        let page = pages.get(page_index).map_err(|e| Error::Pdfium {
            reason: format!("Failed to get page {}: {}", page_index + 1, e),
        })?;

        let mut image_index = 0u32;

        for object in page.objects().iter() {
            if let Some(image_object) = object.as_image_object() {
                // Try to get the processed image (rendered as bitmap)
                if let Ok(dynamic_image) = image_object.get_processed_image(&document) {
                    // Convert to PNG bytes
                    let mut png_bytes = Vec::new();
                    if dynamic_image
                        .write_to(
                            &mut std::io::Cursor::new(&mut png_bytes),
                            image::ImageFormat::Png,
                        )
                        .is_ok()
                    {
                        let base64_data =
                            base64::engine::general_purpose::STANDARD.encode(&png_bytes);

                        images.push(ExtractedImage {
                            page: page_index as u32 + 1,
                            index: image_index,
                            width: dynamic_image.width(),
                            height: dynamic_image.height(),
                            data_base64: base64_data,
                            mime_type: "image/png".to_string(),
                        });

                        image_index += 1;
                    }
                }
            }
        }
    }

    Ok(images)
}

/// Extract images from specific pages
pub fn extract_images_from_pages(
    data: &[u8],
    password: Option<&str>,
    page_numbers: &[u32],
) -> Result<Vec<ExtractedImage>> {
    if data.len() < 4 || &data[0..4] != b"%PDF" {
        return Err(Error::InvalidPdf {
            reason: "Not a valid PDF file".to_string(),
        });
    }

    let pdfium = create_pdfium()?;

    let document = match password {
        Some(pwd) => pdfium.load_pdf_from_byte_slice(data, Some(pwd)),
        None => pdfium.load_pdf_from_byte_slice(data, None),
    }
    .map_err(|e| match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            Error::PasswordRequired
        }
        _ => Error::Pdfium {
            reason: format!("{}", e),
        },
    })?;

    let mut images = Vec::new();
    let pages = document.pages();
    let page_count = pages.len() as u32;

    for &page_num in page_numbers {
        if page_num < 1 || page_num > page_count {
            continue; // Skip invalid page numbers
        }

        let page_index = page_num - 1;
        let page = pages.get(page_index as u16).map_err(|e| Error::Pdfium {
            reason: format!("Failed to get page {}: {}", page_num, e),
        })?;

        let mut image_index = 0u32;

        for object in page.objects().iter() {
            if let Some(image_object) = object.as_image_object() {
                // Try to get the processed image (rendered as bitmap)
                if let Ok(dynamic_image) = image_object.get_processed_image(&document) {
                    // Convert to PNG bytes
                    let mut png_bytes = Vec::new();
                    if dynamic_image
                        .write_to(
                            &mut std::io::Cursor::new(&mut png_bytes),
                            image::ImageFormat::Png,
                        )
                        .is_ok()
                    {
                        let base64_data =
                            base64::engine::general_purpose::STANDARD.encode(&png_bytes);

                        images.push(ExtractedImage {
                            page: page_num,
                            index: image_index,
                            width: dynamic_image.width(),
                            height: dynamic_image.height(),
                            data_base64: base64_data,
                            mime_type: "image/png".to_string(),
                        });

                        image_index += 1;
                    }
                }
            }
        }
    }

    Ok(images)
}

/// Convert PdfPageAnnotationType to string
fn annotation_type_to_string(ann_type: PdfPageAnnotationType) -> String {
    match ann_type {
        PdfPageAnnotationType::Text => "text",
        PdfPageAnnotationType::Link => "link",
        PdfPageAnnotationType::FreeText => "freetext",
        PdfPageAnnotationType::Line => "line",
        PdfPageAnnotationType::Square => "square",
        PdfPageAnnotationType::Circle => "circle",
        PdfPageAnnotationType::Polygon => "polygon",
        PdfPageAnnotationType::Polyline => "polyline",
        PdfPageAnnotationType::Highlight => "highlight",
        PdfPageAnnotationType::Underline => "underline",
        PdfPageAnnotationType::Squiggly => "squiggly",
        PdfPageAnnotationType::Strikeout => "strikeout",
        PdfPageAnnotationType::Stamp => "stamp",
        PdfPageAnnotationType::Caret => "caret",
        PdfPageAnnotationType::Ink => "ink",
        PdfPageAnnotationType::Popup => "popup",
        PdfPageAnnotationType::FileAttachment => "fileattachment",
        PdfPageAnnotationType::Sound => "sound",
        PdfPageAnnotationType::Movie => "movie",
        PdfPageAnnotationType::Widget => "widget",
        PdfPageAnnotationType::Screen => "screen",
        PdfPageAnnotationType::PrinterMark => "printermark",
        PdfPageAnnotationType::TrapNet => "trapnet",
        PdfPageAnnotationType::Watermark => "watermark",
        PdfPageAnnotationType::ThreeD => "threed",
        PdfPageAnnotationType::RichMedia => "richmedia",
        PdfPageAnnotationType::XfaWidget => "xfawidget",
        PdfPageAnnotationType::Redacted => "redacted",
        PdfPageAnnotationType::Unknown => "unknown",
    }
    .to_string()
}

/// Convert string to PdfPageAnnotationType for filtering
fn string_to_annotation_type(s: &str) -> Option<PdfPageAnnotationType> {
    match s.to_lowercase().as_str() {
        "text" => Some(PdfPageAnnotationType::Text),
        "link" => Some(PdfPageAnnotationType::Link),
        "freetext" => Some(PdfPageAnnotationType::FreeText),
        "line" => Some(PdfPageAnnotationType::Line),
        "square" => Some(PdfPageAnnotationType::Square),
        "circle" => Some(PdfPageAnnotationType::Circle),
        "polygon" => Some(PdfPageAnnotationType::Polygon),
        "polyline" => Some(PdfPageAnnotationType::Polyline),
        "highlight" => Some(PdfPageAnnotationType::Highlight),
        "underline" => Some(PdfPageAnnotationType::Underline),
        "squiggly" => Some(PdfPageAnnotationType::Squiggly),
        "strikeout" => Some(PdfPageAnnotationType::Strikeout),
        "stamp" => Some(PdfPageAnnotationType::Stamp),
        "caret" => Some(PdfPageAnnotationType::Caret),
        "ink" => Some(PdfPageAnnotationType::Ink),
        "popup" => Some(PdfPageAnnotationType::Popup),
        "fileattachment" => Some(PdfPageAnnotationType::FileAttachment),
        "sound" => Some(PdfPageAnnotationType::Sound),
        "movie" => Some(PdfPageAnnotationType::Movie),
        "widget" => Some(PdfPageAnnotationType::Widget),
        "screen" => Some(PdfPageAnnotationType::Screen),
        "printermark" => Some(PdfPageAnnotationType::PrinterMark),
        "trapnet" => Some(PdfPageAnnotationType::TrapNet),
        "watermark" => Some(PdfPageAnnotationType::Watermark),
        "threed" => Some(PdfPageAnnotationType::ThreeD),
        "richmedia" => Some(PdfPageAnnotationType::RichMedia),
        "xfawidget" => Some(PdfPageAnnotationType::XfaWidget),
        "redacted" => Some(PdfPageAnnotationType::Redacted),
        _ => None,
    }
}

/// Convert PdfColor to hex string
fn color_to_hex(color: &PdfColor) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        color.red(),
        color.green(),
        color.blue()
    )
}

/// Extract annotations from PDF bytes
pub fn extract_annotations(
    data: &[u8],
    password: Option<&str>,
    page_numbers: Option<&[u32]>,
    annotation_types: Option<&[String]>,
) -> Result<Vec<PdfAnnotation>> {
    if data.len() < 4 || &data[0..4] != b"%PDF" {
        return Err(Error::InvalidPdf {
            reason: "Not a valid PDF file".to_string(),
        });
    }

    let pdfium = create_pdfium()?;

    let document = match password {
        Some(pwd) => pdfium.load_pdf_from_byte_slice(data, Some(pwd)),
        None => pdfium.load_pdf_from_byte_slice(data, None),
    }
    .map_err(|e| match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            Error::PasswordRequired
        }
        _ => Error::Pdfium {
            reason: format!("{}", e),
        },
    })?;

    // Build filter list for annotation types (Vec instead of HashSet since PdfPageAnnotationType doesn't implement Hash)
    let type_filter: Option<Vec<PdfPageAnnotationType>> = annotation_types.map(|types| {
        types
            .iter()
            .filter_map(|s| string_to_annotation_type(s))
            .collect()
    });

    let mut annotations = Vec::new();
    let pages = document.pages();
    let page_count = pages.len() as u32;

    // Determine which pages to process
    let pages_to_process: Vec<u32> = match page_numbers {
        Some(nums) => nums
            .iter()
            .filter(|&&n| n >= 1 && n <= page_count)
            .copied()
            .collect(),
        None => (1..=page_count).collect(),
    };

    for page_num in pages_to_process {
        let page_index = page_num - 1;
        let page = pages.get(page_index as u16).map_err(|e| Error::Pdfium {
            reason: format!("Failed to get page {}: {}", page_num, e),
        })?;

        // Get page text for extracting highlighted text
        let page_text = page.text().ok();

        for annotation in page.annotations().iter() {
            let ann_type = annotation.annotation_type();

            // Filter by annotation type if filter is specified
            if let Some(ref filter) = type_filter {
                if !filter.contains(&ann_type) {
                    continue;
                }
            }

            // Skip popup annotations as they are associated with other annotations
            if ann_type == PdfPageAnnotationType::Popup {
                continue;
            }

            let type_string = annotation_type_to_string(ann_type);

            // Get contents (comment text) - using PdfPageAnnotationCommon trait
            let contents = annotation.contents().filter(|s| !s.is_empty());

            // Get creator (author) - this is the person who created the annotation
            let author = annotation.creator().filter(|s| !s.is_empty());

            // Get creation date
            let created = annotation.creation_date().map(|dt| dt.to_string());

            // Get modification date
            let modified = annotation.modification_date().map(|dt| dt.to_string());

            // Get bounds using accessor methods (not deprecated fields)
            let bounds = annotation.bounds().ok().map(|rect| {
                (
                    rect.left().value,
                    rect.top().value,
                    rect.right().value,
                    rect.bottom().value,
                )
            });

            // Get fill color
            let color = annotation.fill_color().ok().map(|c| color_to_hex(&c));

            // Try to extract highlighted text for text markup annotations
            let highlighted_text = if matches!(
                ann_type,
                PdfPageAnnotationType::Highlight
                    | PdfPageAnnotationType::Underline
                    | PdfPageAnnotationType::Squiggly
                    | PdfPageAnnotationType::Strikeout
            ) {
                extract_text_for_annotation(&page_text, &annotation)
            } else {
                None
            };

            annotations.push(PdfAnnotation {
                page: page_num,
                annotation_type: type_string,
                contents,
                author,
                created,
                modified,
                bounds,
                highlighted_text,
                color,
            });
        }
    }

    Ok(annotations)
}

/// Extract text that falls within the annotation bounds
fn extract_text_for_annotation(
    page_text: &Option<PdfPageText>,
    annotation: &PdfPageAnnotation,
) -> Option<String> {
    let page_text = page_text.as_ref()?;

    // Try to get text for the annotation using pdfium's built-in method
    if let Ok(text) = page_text.for_annotation(annotation) {
        let text_str = text.trim().to_string();
        if !text_str.is_empty() {
            return Some(text_str);
        }
    }

    // Fallback: use bounds to extract text
    let bounds = annotation.bounds().ok()?;
    let text = page_text.inside_rect(bounds);
    let text_str = text.trim().to_string();
    if !text_str.is_empty() {
        return Some(text_str);
    }

    None
}

/// Extract links from PDF bytes
/// Extracts URL links and internal page navigation links from link annotations
pub fn extract_links(
    data: &[u8],
    password: Option<&str>,
    page_numbers: Option<&[u32]>,
) -> Result<Vec<PdfLink>> {
    if data.len() < 4 || &data[0..4] != b"%PDF" {
        return Err(Error::InvalidPdf {
            reason: "Not a valid PDF file".to_string(),
        });
    }

    let pdfium = create_pdfium()?;

    let document = match password {
        Some(pwd) => pdfium.load_pdf_from_byte_slice(data, Some(pwd)),
        None => pdfium.load_pdf_from_byte_slice(data, None),
    }
    .map_err(|e| match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            Error::PasswordRequired
        }
        _ => Error::Pdfium {
            reason: format!("{}", e),
        },
    })?;

    let mut links = Vec::new();
    let pages = document.pages();
    let page_count = pages.len() as u32;

    // Determine which pages to process
    let pages_to_process: Vec<u32> = match page_numbers {
        Some(nums) => nums
            .iter()
            .filter(|&&n| n >= 1 && n <= page_count)
            .copied()
            .collect(),
        None => (1..=page_count).collect(),
    };

    for page_num in pages_to_process {
        let page_index = page_num - 1;
        let page = pages.get(page_index as u16).map_err(|e| Error::Pdfium {
            reason: format!("Failed to get page {}: {}", page_num, e),
        })?;

        // Get page text for extracting link text
        let page_text = page.text().ok();

        // Extract links from the page's link collection
        for link in page.links().iter() {
            let mut url = None;
            let mut dest_page = None;

            // Try to get action (for URI links)
            if let Some(action) = link.action() {
                if action.action_type() == pdfium_render::prelude::PdfActionType::Uri {
                    if let Some(uri_action) = action.as_uri_action() {
                        url = Some(uri_action.uri().unwrap_or_default());
                    }
                }
            }

            // Try to get destination (for internal links)
            if url.is_none() {
                if let Some(dest) = link.destination() {
                    if let Ok(page_idx) = dest.page_index() {
                        dest_page = Some(page_idx as u32 + 1);
                    }
                }
            }

            // Skip links with no URL or destination
            if url.is_none() && dest_page.is_none() {
                continue;
            }

            links.push(PdfLink {
                page: page_num,
                url,
                dest_page,
                bounds: None, // PdfLink doesn't expose bounds directly
                text: None,   // We'll try to match with annotations below
            });
        }

        // Also extract link annotations which have bounds
        for annotation in page.annotations().iter() {
            if annotation.annotation_type() != PdfPageAnnotationType::Link {
                continue;
            }

            // Get bounds using PdfPageAnnotationCommon trait
            let bounds = annotation.bounds().ok().map(|rect| {
                (
                    rect.left().value,
                    rect.top().value,
                    rect.right().value,
                    rect.bottom().value,
                )
            });

            // Try to extract text from annotation area
            let text = if let (Some(ref pt), Some(b)) = (&page_text, &bounds) {
                let rect = pdfium_render::prelude::PdfRect::new_from_values(b.0, b.3, b.2, b.1);
                let link_text = pt.inside_rect(rect);
                let trimmed = link_text.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            } else {
                None
            };

            // Update existing link with bounds/text if we can match, or add new entry
            // Match by checking if a link on this page has no bounds yet
            let mut matched = false;
            for existing in &mut links {
                if existing.page == page_num && existing.bounds.is_none() {
                    existing.bounds = bounds;
                    existing.text = text.clone();
                    matched = true;
                    break;
                }
            }

            // If no match and we have bounds, add as a link annotation
            // (some link annotations may not have corresponding link objects)
            if !matched && bounds.is_some() {
                links.push(PdfLink {
                    page: page_num,
                    url: None,
                    dest_page: None,
                    bounds,
                    text,
                });
            }
        }
    }

    // Remove links that have no useful information
    links.retain(|l| l.url.is_some() || l.dest_page.is_some() || l.text.is_some());

    Ok(links)
}

/// Get page information from PDF bytes
pub fn get_page_info(data: &[u8], password: Option<&str>) -> Result<Vec<PdfPageInfo>> {
    if data.len() < 4 || &data[0..4] != b"%PDF" {
        return Err(Error::InvalidPdf {
            reason: "Not a valid PDF file".to_string(),
        });
    }

    let pdfium = create_pdfium()?;

    let document = match password {
        Some(pwd) => pdfium.load_pdf_from_byte_slice(data, Some(pwd)),
        None => pdfium.load_pdf_from_byte_slice(data, None),
    }
    .map_err(|e| match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            Error::PasswordRequired
        }
        _ => Error::Pdfium {
            reason: format!("{}", e),
        },
    })?;

    let mut page_infos = Vec::new();
    let pages = document.pages();

    for page_index in 0..pages.len() {
        let page = pages.get(page_index).map_err(|e| Error::Pdfium {
            reason: format!("Failed to get page {}: {}", page_index + 1, e),
        })?;

        let width = page.width().value;
        let height = page.height().value;

        // Get rotation (returns PdfPageRenderRotation)
        let rotation = match page.rotation() {
            Ok(rot) => match rot {
                pdfium_render::prelude::PdfPageRenderRotation::None => 0,
                pdfium_render::prelude::PdfPageRenderRotation::Degrees90 => 90,
                pdfium_render::prelude::PdfPageRenderRotation::Degrees180 => 180,
                pdfium_render::prelude::PdfPageRenderRotation::Degrees270 => 270,
            },
            Err(_) => 0,
        };

        // Determine orientation based on dimensions (accounting for rotation)
        let effective_width = if rotation == 90 || rotation == 270 {
            height
        } else {
            width
        };
        let effective_height = if rotation == 90 || rotation == 270 {
            width
        } else {
            height
        };
        let orientation = if effective_width > effective_height {
            "landscape"
        } else if effective_width < effective_height {
            "portrait"
        } else {
            "square"
        }
        .to_string();

        // Get text statistics: character count, word count, and estimated tokens
        let (char_count, word_count, estimated_token_count) = page
            .text()
            .ok()
            .map(|t| {
                let mut total_chars = 0usize;
                let mut cjk_chars = 0usize;
                let mut other_chars = 0usize;
                let mut text_content = String::new();

                for seg in t.segments().iter() {
                    if let Ok(chars) = seg.chars() {
                        for char_obj in chars.iter() {
                            if let Some(c) = char_obj.unicode_char() {
                                total_chars += 1;
                                text_content.push(c);
                                // CJK character ranges:
                                // - CJK Unified Ideographs: U+4E00-U+9FFF
                                // - CJK Extension A: U+3400-U+4DBF
                                // - Hiragana: U+3040-U+309F
                                // - Katakana: U+30A0-U+30FF
                                // - Hangul: U+AC00-U+D7AF
                                // - Fullwidth characters: U+FF00-U+FFEF
                                let code = c as u32;
                                if (0x4E00..=0x9FFF).contains(&code)
                                    || (0x3400..=0x4DBF).contains(&code)
                                    || (0x3040..=0x309F).contains(&code)
                                    || (0x30A0..=0x30FF).contains(&code)
                                    || (0xAC00..=0xD7AF).contains(&code)
                                    || (0xFF00..=0xFFEF).contains(&code)
                                    || (0x20000..=0x2A6DF).contains(&code)
                                {
                                    cjk_chars += 1;
                                } else {
                                    other_chars += 1;
                                }
                            }
                        }
                    }
                }

                // Word count: split by whitespace
                let words = text_content.split_whitespace().count();

                // Token estimation (conservative):
                // NOTE: Actual token counts vary significantly by model (GPT, Claude, etc.)
                // This is a rough approximation for planning purposes only.
                // - CJK: ~2 tokens per character (common chars: 1-2, rare chars: 2-3)
                // - Latin/other: ~4 characters per token
                let cjk_tokens = cjk_chars * 2; // 2 tokens per CJK char (conservative)
                let other_tokens = other_chars.div_ceil(4); // 4 chars per token for latin

                (total_chars, words, cjk_tokens + other_tokens)
            })
            .unwrap_or((0, 0, 0));

        page_infos.push(PdfPageInfo {
            page: page_index as u32 + 1,
            width,
            height,
            rotation,
            orientation,
            char_count,
            word_count,
            estimated_token_count,
        });
    }

    Ok(page_infos)
}

/// Parse page range string (e.g., "1-5,10,15-20")
pub fn parse_page_range(range: &str, max_pages: u32) -> Result<Vec<u32>> {
    let mut pages = Vec::new();

    for part in range.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if let Some((start, end)) = part.split_once('-') {
            let start: u32 = start.trim().parse().map_err(|_| Error::InvalidPageRange {
                range: range.to_string(),
            })?;
            let end: u32 = end.trim().parse().map_err(|_| Error::InvalidPageRange {
                range: range.to_string(),
            })?;

            if start < 1 || end > max_pages || start > end {
                return Err(Error::InvalidPageRange {
                    range: range.to_string(),
                });
            }

            for page in start..=end {
                pages.push(page);
            }
        } else {
            let page: u32 = part.parse().map_err(|_| Error::InvalidPageRange {
                range: range.to_string(),
            })?;

            if page < 1 || page > max_pages {
                return Err(Error::InvalidPageRange {
                    range: range.to_string(),
                });
            }

            pages.push(page);
        }
    }

    // Remove duplicates and sort
    pages.sort();
    pages.dedup();

    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_pdf_detection() {
        let result = PdfReader::open_bytes(b"not a pdf", None);
        assert!(matches!(result, Err(Error::InvalidPdf { .. })));
    }

    #[test]
    fn test_parse_page_range() {
        assert_eq!(parse_page_range("1-3", 10).unwrap(), vec![1, 2, 3]);
        assert_eq!(parse_page_range("1,3,5", 10).unwrap(), vec![1, 3, 5]);
        assert_eq!(
            parse_page_range("1-3,5,7-9", 10).unwrap(),
            vec![1, 2, 3, 5, 7, 8, 9]
        );
        assert_eq!(parse_page_range("1,1,2,2", 10).unwrap(), vec![1, 2]); // Dedup
    }

    #[test]
    fn test_parse_page_range_invalid() {
        assert!(parse_page_range("0-3", 10).is_err()); // 0 is invalid
        assert!(parse_page_range("1-15", 10).is_err()); // Out of bounds
        assert!(parse_page_range("5-3", 10).is_err()); // Start > End
        assert!(parse_page_range("abc", 10).is_err()); // Not a number
    }
}
