//! PDF reader wrapper for PDFium

use crate::error::{Error, Result};
use base64::Engine;
use pdfium_render::prelude::*;
use std::collections::HashMap;
use std::path::Path;

// ============================================================================
// LLM-Optimized Text Extraction Types
// ============================================================================

/// Character information for advanced text extraction
#[derive(Debug, Clone)]
pub struct CharInfo {
    /// The character
    pub char: char,
    /// X coordinate (left)
    pub x: f32,
    /// Y coordinate (top)
    pub y: f32,
    /// Character width
    pub width: f32,
    /// Character height (used for font size estimation)
    pub height: f32,
}

/// Line information after grouping characters
#[derive(Debug, Clone)]
pub struct LineInfo {
    /// Characters in this line with their X positions
    pub chars: Vec<(char, f32)>,
    /// Y coordinate of the line (top)
    pub y: f32,
    /// Average character height (font size proxy)
    pub avg_height: f32,
    /// Minimum X coordinate (leftmost character)
    pub min_x: f32,
    /// Maximum X coordinate (rightmost character)
    pub max_x: f32,
}

/// Configuration for text extraction (LLM-optimized by default)
#[derive(Debug, Clone)]
pub struct TextExtractionConfig {
    /// Paragraph mode: "none" or "spacing"
    pub paragraph_mode: String,
    /// Line spacing multiplier for paragraph detection
    pub paragraph_threshold: f32,
    /// Column mode: "none" or "auto"
    pub column_mode: String,
    /// Minimum gap for column detection
    pub column_gap: f32,
    /// Watermark mode: "none" or "center"
    pub watermark_mode: String,
    /// Use dynamic thresholds based on font size
    pub dynamic_thresholds: bool,
    /// Page dimensions (set per-page during extraction)
    pub page_width: f32,
    pub page_height: f32,
}

impl Default for TextExtractionConfig {
    /// Create a default config with LLM-optimized settings
    fn default() -> Self {
        Self {
            paragraph_mode: "spacing".to_string(),
            paragraph_threshold: 1.5,
            column_mode: "auto".to_string(),
            column_gap: 30.0,
            watermark_mode: "center".to_string(),
            dynamic_thresholds: true,
            page_width: 0.0,
            page_height: 0.0,
        }
    }
}

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
    /// Number of image objects on this page
    pub image_count: usize,
    /// Number of annotations on this page (excluding form field widgets)
    pub annotation_count: usize,
    /// Number of links on this page
    pub link_count: usize,
    /// Number of form fields on this page
    pub form_field_count: usize,
    /// Form field types on this page (e.g., "text", "checkbox")
    pub form_field_types: Vec<String>,
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

// ============================================================================
// Page Rendering Types
// ============================================================================

/// Rendered page image data
#[derive(Debug, Clone)]
pub struct RenderedPage {
    /// Page number (1-indexed)
    pub page: u32,
    /// Image width in pixels
    pub width: u32,
    /// Image height in pixels
    pub height: u32,
    /// Base64-encoded PNG image data
    pub data_base64: String,
    /// MIME type (always "image/png")
    pub mime_type: String,
}

// ============================================================================
// Form Field Types
// ============================================================================

/// Information about a PDF form field
#[derive(Debug, Clone)]
pub struct FormFieldInfo {
    /// Page number (1-indexed)
    pub page: u32,
    /// Field name
    pub name: Option<String>,
    /// Field type (e.g., "text", "checkbox", "radio_button", "combo_box", "list_box", "push_button", "signature", "unknown")
    pub field_type: String,
    /// Current value (for text fields)
    pub value: Option<String>,
    /// Whether checked (for checkbox/radio)
    pub is_checked: Option<bool>,
    /// Read-only flag
    pub is_read_only: bool,
    /// Required flag
    pub is_required: bool,
    /// Available options (for combo_box/list_box)
    pub options: Option<Vec<FormFieldOptionInfo>>,
    /// Type-specific properties
    pub properties: FormFieldProperties,
}

/// Option info for combo/list box fields
#[derive(Debug, Clone)]
pub struct FormFieldOptionInfo {
    /// Display label
    pub label: Option<String>,
    /// Whether this option is currently selected
    pub is_selected: bool,
}

/// Type-specific form field properties
#[derive(Debug, Clone, Default)]
pub struct FormFieldProperties {
    /// Whether text field is multiline
    pub is_multiline: Option<bool>,
    /// Whether text field is a password field
    pub is_password: Option<bool>,
    /// Whether combo box is editable
    pub is_editable: Option<bool>,
    /// Whether combo/list box allows multiple selections
    pub is_multiselect: Option<bool>,
}

/// Value to set on a form field
#[derive(Debug, Clone)]
pub struct FormFieldValue {
    /// Field name to match
    pub name: String,
    /// Text value (for text fields)
    pub value: Option<String>,
    /// Checked state (for checkbox/radio)
    pub checked: Option<bool>,
}

/// Result of filling form fields
#[derive(Debug, Clone)]
pub struct FillFormResultInfo {
    /// Number of fields successfully filled
    pub fields_filled: u32,
    /// Fields that could not be filled
    pub fields_skipped: Vec<SkippedField>,
}

/// Info about a field that could not be filled
#[derive(Debug, Clone)]
pub struct SkippedField {
    /// Field name
    pub name: String,
    /// Reason the field was skipped
    pub reason: String,
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

    /// Extract text from a page with LLM-optimized options
    pub fn extract_page_text_with_options(
        page: &PdfPage,
        config: &TextExtractionConfig,
    ) -> Result<String> {
        let text_obj = match page.text() {
            Ok(t) => t,
            Err(_) => return Ok(String::new()),
        };

        // Step 1: Collect all characters with full info (position, size)
        let chars = Self::collect_chars_with_info(&text_obj);
        if chars.is_empty() {
            return Ok(String::new());
        }

        // Step 2: Calculate thresholds (dynamic or fixed)
        let (y_tolerance, space_threshold) = if config.dynamic_thresholds {
            Self::calculate_dynamic_thresholds(&chars)
        } else {
            (5.0, 10.0) // Legacy fixed values
        };

        // Step 3: Group characters into lines
        let mut lines = Self::group_into_lines(chars, y_tolerance);

        // Step 4: Filter watermarks if enabled
        if config.watermark_mode == "center" {
            lines = Self::filter_watermarks(lines, config);
        }

        // Step 5: Handle column detection
        let lines = if config.column_mode == "auto" {
            Self::reorder_columns(lines, config.column_gap)
        } else {
            lines
        };

        // Step 6: Build text output with paragraph detection
        let result = Self::build_text_output(&lines, space_threshold, config);

        Ok(result)
    }

    /// Collect character information from page text
    fn collect_chars_with_info(text_obj: &PdfPageText) -> Vec<CharInfo> {
        let mut chars = Vec::new();

        for segment in text_obj.segments().iter() {
            if let Ok(char_iter) = segment.chars() {
                for char_result in char_iter.iter() {
                    if let Some(c) = char_result.unicode_char() {
                        if let Ok(bounds) = char_result.loose_bounds() {
                            let x = bounds.left().value;
                            let y = bounds.top().value;
                            let width = bounds.width().value;
                            let height = bounds.height().value;

                            chars.push(CharInfo {
                                char: c,
                                x,
                                y,
                                width,
                                height,
                            });
                        }
                    }
                }
            }
        }

        chars
    }

    /// Calculate dynamic thresholds based on font size distribution
    fn calculate_dynamic_thresholds(chars: &[CharInfo]) -> (f32, f32) {
        if chars.is_empty() {
            return (5.0, 10.0); // Fallback to defaults
        }

        // Calculate median height as the representative font size
        let mut heights: Vec<f32> = chars
            .iter()
            .filter(|c| c.height > 0.0)
            .map(|c| c.height)
            .collect();

        if heights.is_empty() {
            return (5.0, 10.0);
        }

        heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median_height = heights[heights.len() / 2];

        // Y tolerance: approximately 40% of median font height
        // This accounts for baseline variations within the same line
        let y_tolerance = median_height * 0.4;

        // Space threshold: approximately 30% of median font height
        // Characters with larger gaps are separated by spaces
        let space_threshold = median_height * 0.3;

        (y_tolerance.max(2.0), space_threshold.max(3.0))
    }

    /// Group characters into lines based on Y-coordinate proximity
    fn group_into_lines(chars: Vec<CharInfo>, y_tolerance: f32) -> Vec<LineInfo> {
        if chars.is_empty() {
            return Vec::new();
        }

        // Sort by Y descending (top to bottom), then X ascending
        let mut sorted_chars = chars;
        sorted_chars.sort_by(|a, b| {
            let y_cmp = b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal);
            if y_cmp == std::cmp::Ordering::Equal {
                a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal)
            } else {
                y_cmp
            }
        });

        let mut lines: Vec<LineInfo> = Vec::new();
        let mut current_chars: Vec<CharInfo> = Vec::new();
        let mut current_y: Option<f32> = None;

        for char_info in sorted_chars {
            match current_y {
                Some(cur_y) if (cur_y - char_info.y).abs() <= y_tolerance => {
                    current_chars.push(char_info);
                }
                _ => {
                    if !current_chars.is_empty() {
                        lines.push(Self::create_line_info(current_chars));
                    }
                    current_y = Some(char_info.y);
                    current_chars = vec![char_info];
                }
            }
        }

        if !current_chars.is_empty() {
            lines.push(Self::create_line_info(current_chars));
        }

        lines
    }

    /// Create LineInfo from a collection of characters
    fn create_line_info(chars: Vec<CharInfo>) -> LineInfo {
        let avg_height = if chars.is_empty() {
            0.0
        } else {
            chars.iter().map(|c| c.height).sum::<f32>() / chars.len() as f32
        };

        let min_x = chars.iter().map(|c| c.x).fold(f32::MAX, f32::min);
        let max_x = chars.iter().map(|c| c.x + c.width).fold(f32::MIN, f32::max);
        let y = chars.first().map(|c| c.y).unwrap_or(0.0);

        LineInfo {
            chars: chars.into_iter().map(|c| (c.char, c.x)).collect(),
            y,
            avg_height,
            min_x,
            max_x,
        }
    }

    /// Filter out watermarks (centered large text)
    fn filter_watermarks(lines: Vec<LineInfo>, config: &TextExtractionConfig) -> Vec<LineInfo> {
        if config.page_width <= 0.0 {
            return lines;
        }

        // Calculate average font height across all lines
        let avg_font_height: f32 = if lines.is_empty() {
            12.0
        } else {
            lines.iter().map(|l| l.avg_height).sum::<f32>() / lines.len() as f32
        };

        let page_center = config.page_width / 2.0;
        let center_tolerance = config.page_width * 0.2; // 20% tolerance for centering

        lines
            .into_iter()
            .filter(|line| {
                // Check if line is approximately centered
                let line_center = (line.min_x + line.max_x) / 2.0;
                let is_centered = (line_center - page_center).abs() < center_tolerance;

                // Check if text is significantly larger than average (1.5x or more)
                let is_large = line.avg_height > avg_font_height * 1.5;

                // Check if it's a short line (watermarks are usually short)
                let is_short = line.chars.len() < 30;

                // Filter out if it looks like a watermark
                !(is_centered && is_large && is_short)
            })
            .collect()
    }

    /// Detect and reorder columns
    fn reorder_columns(lines: Vec<LineInfo>, column_gap: f32) -> Vec<LineInfo> {
        if lines.is_empty() {
            return lines;
        }

        // Detect potential column boundaries by analyzing X-coordinate gaps
        let mut all_x_positions: Vec<f32> = lines
            .iter()
            .flat_map(|line| line.chars.iter().map(|(_, x)| *x))
            .collect();
        all_x_positions.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Find significant gaps that could indicate column separators
        let mut gaps: Vec<(f32, f32)> = Vec::new(); // (gap_start, gap_end)
        for window in all_x_positions.windows(2) {
            let gap = window[1] - window[0];
            if gap >= column_gap {
                gaps.push((window[0], window[1]));
            }
        }

        // If no significant gaps found, return as-is
        if gaps.is_empty() {
            return lines;
        }

        // Find the most prominent column separator (largest gap that appears consistently)
        // Group gaps by position and find the one that appears most frequently
        let mut gap_histogram: HashMap<i32, (f32, usize)> = HashMap::new();
        for (start, end) in &gaps {
            let mid = ((start + end) / 2.0) as i32;
            // Round to nearest 10 points for grouping
            let bucket = (mid / 10) * 10;
            let entry = gap_histogram.entry(bucket).or_insert((0.0, 0));
            entry.0 += end - start;
            entry.1 += 1;
        }

        // Find the bucket with the most occurrences
        let best_separator = gap_histogram
            .iter()
            .max_by_key(|(_, (_, count))| *count)
            .map(|(bucket, _)| *bucket as f32);

        let separator = match best_separator {
            Some(s) => s,
            None => return lines,
        };

        // Split lines into columns
        let mut left_column: Vec<LineInfo> = Vec::new();
        let mut right_column: Vec<LineInfo> = Vec::new();

        for line in lines {
            let line_center = (line.min_x + line.max_x) / 2.0;
            if line_center < separator {
                left_column.push(line);
            } else {
                right_column.push(line);
            }
        }

        // Merge: left column first, then right column
        let mut result = left_column;
        result.extend(right_column);
        result
    }

    /// Build the final text output with optional paragraph detection
    fn build_text_output(
        lines: &[LineInfo],
        space_threshold: f32,
        config: &TextExtractionConfig,
    ) -> String {
        let mut result = String::new();
        let mut prev_y: Option<f32> = None;
        let mut prev_avg_height: Option<f32> = None;

        for line in lines {
            // Sort characters by X position
            let mut sorted_chars: Vec<(char, f32)> = line.chars.clone();
            sorted_chars.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            // Paragraph detection
            if config.paragraph_mode == "spacing" {
                if let (Some(py), Some(ph)) = (prev_y, prev_avg_height) {
                    let line_gap = py - line.y;
                    let normal_gap = ph.max(line.avg_height);

                    // If gap is significantly larger than normal line height, add paragraph break
                    if line_gap > normal_gap * config.paragraph_threshold {
                        result.push('\n');
                    }
                }
            }

            // Build line text
            let mut prev_x: Option<f32> = None;
            for (c, x) in sorted_chars {
                if let Some(px) = prev_x {
                    if x - px > space_threshold && c != ' ' {
                        result.push(' ');
                    }
                }
                result.push(c);
                prev_x = Some(x);
            }

            result.push('\n');
            prev_y = Some(line.y);
            prev_avg_height = Some(line.avg_height);
        }

        result.trim_end().to_string()
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

/// Extract text from PDF bytes with LLM-optimized options
/// Returns Vec of (page_number, text) tuples for specified pages
pub fn extract_text_with_options(
    data: &[u8],
    password: Option<&str>,
    page_numbers: Option<&[u32]>,
    config: &TextExtractionConfig,
) -> Result<Vec<(u32, String)>> {
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

    let mut results = Vec::with_capacity(pages_to_process.len());

    for page_num in pages_to_process {
        let page_index = page_num - 1;
        let page = pages.get(page_index as u16).map_err(|e| Error::Pdfium {
            reason: format!("Failed to get page {}: {}", page_num, e),
        })?;

        // Create config with page dimensions
        let mut page_config = config.clone();
        page_config.page_width = page.width().value;
        page_config.page_height = page.height().value;

        let text = PdfReader::extract_page_text_with_options(&page, &page_config)?;
        results.push((page_num, text));
    }

    Ok(results)
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

        // Count image objects on this page
        let image_count = page
            .objects()
            .iter()
            .filter(|obj| obj.as_image_object().is_some())
            .count();

        // Count annotations, links, and form fields on this page
        let mut annotation_count = 0usize;
        let mut form_field_count = 0usize;
        let mut form_field_types = Vec::new();
        for annotation in page.annotations().iter() {
            if let Some(field) = annotation.as_form_field() {
                form_field_count += 1;
                let ft = format!("{:?}", field.field_type()).to_lowercase();
                form_field_types.push(ft);
            } else {
                let ann_type = annotation.annotation_type();
                // Skip popup annotations (associated with other annotations)
                if ann_type != pdfium_render::prelude::PdfPageAnnotationType::Popup {
                    annotation_count += 1;
                }
            }
        }

        let link_count = page.links().len();

        page_infos.push(PdfPageInfo {
            page: page_index as u32 + 1,
            width,
            height,
            rotation,
            orientation,
            char_count,
            word_count,
            estimated_token_count,
            image_count,
            annotation_count,
            link_count,
            form_field_count,
            form_field_types,
        });
    }

    Ok(page_infos)
}

/// Render PDF pages as PNG images, returned as base64-encoded strings
pub fn render_pages_to_images(
    data: &[u8],
    password: Option<&str>,
    page_numbers: &[u32],
    width: Option<u16>,
    height: Option<u16>,
    scale: Option<f32>,
) -> Result<Vec<RenderedPage>> {
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

    let pages = document.pages();
    let page_count = pages.len() as u32;
    let mut rendered = Vec::new();

    for &page_num in page_numbers {
        if page_num < 1 || page_num > page_count {
            continue;
        }

        let page_index = page_num - 1;
        let page = pages.get(page_index as u16).map_err(|e| Error::Pdfium {
            reason: format!("Failed to get page {}: {}", page_num, e),
        })?;

        // Build render config based on parameters
        let config = if let Some(s) = scale {
            PdfRenderConfig::new().scale_page_by_factor(s)
        } else if let (Some(w), Some(h)) = (width, height) {
            PdfRenderConfig::new().set_target_size(w as i32, h as i32)
        } else if let Some(w) = width {
            PdfRenderConfig::new().set_target_width(w as i32)
        } else if let Some(h) = height {
            PdfRenderConfig::new().set_target_height(h as i32)
        } else {
            // Default: 1200px width
            PdfRenderConfig::new().set_target_width(1200)
        };

        let config = config.render_form_data(true).render_annotations(true);

        let bitmap = page
            .render_with_config(&config)
            .map_err(|e| Error::Pdfium {
                reason: format!("Failed to render page {}: {}", page_num, e),
            })?;

        let dynamic_image = bitmap.as_image();
        let img_width = dynamic_image.width();
        let img_height = dynamic_image.height();

        // Encode as PNG
        let mut png_bytes = Vec::new();
        dynamic_image
            .write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )
            .map_err(|e| Error::Pdfium {
                reason: format!("Failed to encode page {} as PNG: {}", page_num, e),
            })?;

        let base64_data = base64::engine::general_purpose::STANDARD.encode(&png_bytes);

        rendered.push(RenderedPage {
            page: page_num,
            width: img_width,
            height: img_height,
            data_base64: base64_data,
            mime_type: "image/png".to_string(),
        });
    }

    Ok(rendered)
}

/// Extract form fields from PDF bytes
pub fn extract_form_fields(
    data: &[u8],
    password: Option<&str>,
    page_numbers: Option<&[u32]>,
) -> Result<Vec<FormFieldInfo>> {
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

    let mut fields = Vec::new();
    let pages = document.pages();
    let page_count = pages.len() as u32;

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

        for annotation in page.annotations().iter() {
            if let Some(field) = annotation.as_form_field() {
                let mut info = FormFieldInfo {
                    page: page_num,
                    name: field.name(),
                    field_type: String::new(),
                    value: None,
                    is_checked: None,
                    is_read_only: false,
                    is_required: false,
                    options: None,
                    properties: FormFieldProperties::default(),
                };

                if let Some(text_field) = field.as_text_field() {
                    info.field_type = "text".to_string();
                    info.value = text_field.value();
                } else if let Some(checkbox) = field.as_checkbox_field() {
                    info.field_type = "checkbox".to_string();
                    info.is_checked = checkbox.is_checked().ok();
                } else if let Some(radio) = field.as_radio_button_field() {
                    info.field_type = "radio_button".to_string();
                    info.is_checked = radio.is_checked().ok();
                } else if let Some(combo) = field.as_combo_box_field() {
                    info.field_type = "combo_box".to_string();
                    let mut opts = Vec::new();
                    for i in 0..combo.options().len() {
                        if let Ok(opt) = combo.options().get(i) {
                            opts.push(FormFieldOptionInfo {
                                label: opt.label().cloned(),
                                is_selected: opt.is_set(),
                            });
                        }
                    }
                    if !opts.is_empty() {
                        info.options = Some(opts);
                    }
                } else if let Some(list) = field.as_list_box_field() {
                    info.field_type = "list_box".to_string();
                    let mut opts = Vec::new();
                    for i in 0..list.options().len() {
                        if let Ok(opt) = list.options().get(i) {
                            opts.push(FormFieldOptionInfo {
                                label: opt.label().cloned(),
                                is_selected: opt.is_set(),
                            });
                        }
                    }
                    if !opts.is_empty() {
                        info.options = Some(opts);
                    }
                } else if field.as_push_button_field().is_some() {
                    info.field_type = "push_button".to_string();
                } else if field.as_signature_field().is_some() {
                    info.field_type = "signature".to_string();
                } else {
                    info.field_type = "unknown".to_string();
                }

                fields.push(info);
            }
        }
    }

    Ok(fields)
}

/// Fill form fields in a PDF and return the modified PDF bytes
pub fn fill_form_fields(
    data: &[u8],
    password: Option<&str>,
    field_values: &[FormFieldValue],
) -> Result<(Vec<u8>, FillFormResultInfo)> {
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

    let mut fields_filled = 0u32;
    let mut fields_skipped = Vec::new();
    let mut remaining: Vec<&FormFieldValue> = field_values.iter().collect();

    let pages = document.pages();

    for page_index in 0..pages.len() {
        if remaining.is_empty() {
            break;
        }

        let page = pages.get(page_index).map_err(|e| Error::Pdfium {
            reason: format!("Failed to get page {}: {}", page_index + 1, e),
        })?;

        for mut annotation in page.annotations().iter() {
            if remaining.is_empty() {
                break;
            }

            if let Some(field) = annotation.as_form_field_mut() {
                let field_name = field.name();

                // Find matching field value
                let matching_idx = remaining
                    .iter()
                    .position(|fv| field_name.as_ref().map(|n| n == &fv.name).unwrap_or(false));

                if let Some(idx) = matching_idx {
                    let fv = remaining.remove(idx);

                    if let Some(text_field) = field.as_text_field_mut() {
                        if let Some(ref val) = fv.value {
                            match text_field.set_value(val) {
                                Ok(_) => {
                                    fields_filled += 1;
                                }
                                Err(e) => {
                                    fields_skipped.push(SkippedField {
                                        name: fv.name.clone(),
                                        reason: format!("Failed to set value: {}", e),
                                    });
                                }
                            }
                        } else {
                            fields_skipped.push(SkippedField {
                                name: fv.name.clone(),
                                reason: "Text field requires 'value' parameter".to_string(),
                            });
                        }
                    } else if let Some(checkbox) = field.as_checkbox_field_mut() {
                        if let Some(checked) = fv.checked {
                            match checkbox.set_checked(checked) {
                                Ok(_) => {
                                    fields_filled += 1;
                                }
                                Err(e) => {
                                    fields_skipped.push(SkippedField {
                                        name: fv.name.clone(),
                                        reason: format!("Failed to set checked: {}", e),
                                    });
                                }
                            }
                        } else {
                            fields_skipped.push(SkippedField {
                                name: fv.name.clone(),
                                reason: "Checkbox field requires 'checked' parameter".to_string(),
                            });
                        }
                    } else if let Some(radio) = field.as_radio_button_field_mut() {
                        if let Some(true) = fv.checked {
                            match radio.set_checked() {
                                Ok(_) => {
                                    fields_filled += 1;
                                }
                                Err(e) => {
                                    fields_skipped.push(SkippedField {
                                        name: fv.name.clone(),
                                        reason: format!("Failed to select radio: {}", e),
                                    });
                                }
                            }
                        } else {
                            fields_skipped.push(SkippedField {
                                name: fv.name.clone(),
                                reason: "Radio button requires 'checked: true' to select"
                                    .to_string(),
                            });
                        }
                    } else {
                        fields_skipped.push(SkippedField {
                            name: fv.name.clone(),
                            reason: "Unsupported field type for writing".to_string(),
                        });
                    }
                }
            }
        }
    }

    // Report any remaining unmatched field names
    for fv in remaining {
        fields_skipped.push(SkippedField {
            name: fv.name.clone(),
            reason: "Field not found in PDF".to_string(),
        });
    }

    // Save the modified PDF to bytes
    let output_bytes = document.save_to_bytes().map_err(|e| Error::Pdfium {
        reason: format!("Failed to save modified PDF: {}", e),
    })?;

    Ok((
        output_bytes,
        FillFormResultInfo {
            fields_filled,
            fields_skipped,
        },
    ))
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
