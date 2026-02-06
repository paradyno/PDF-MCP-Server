//! PDF processing layer
//!
//! This module provides PDF processing functionality using PDFium and qpdf.

mod qpdf;
mod reader;

pub use qpdf::QpdfWrapper;
pub use reader::{
    extract_annotations, extract_images, extract_images_from_pages, extract_links, get_page_info,
    parse_page_range, ExtractedImage, OutlineItem, PdfAnnotation, PdfLink, PdfMetadataInfo,
    PdfPageInfo, PdfReader,
};
