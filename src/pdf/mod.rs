//! PDF processing layer
//!
//! This module provides PDF processing functionality using PDFium and qpdf.

mod qpdf;
mod reader;

pub use qpdf::QpdfWrapper;
pub use reader::{
    extract_annotations, extract_form_fields, extract_images, extract_images_from_pages,
    extract_links, extract_text_with_options, fill_form_fields, get_page_info, parse_page_range,
    render_pages_to_images, CharInfo, ExtractedImage, FillFormResultInfo, FormFieldInfo,
    FormFieldOptionInfo, FormFieldProperties, FormFieldValue, LineInfo, OutlineItem, PdfAnnotation,
    PdfLink, PdfMetadataInfo, PdfPageInfo, PdfReader, RenderedPage, SkippedField,
    TextExtractionConfig,
};
