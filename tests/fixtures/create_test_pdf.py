#!/usr/bin/env python3
"""
Create a CC0 test PDF with bookmarks/outlines and images.

This script generates a PDF file that contains:
- Multiple pages with hierarchical bookmarks/outlines
- Embedded images (simple geometric shapes)
- Text content for testing extraction

The resulting PDF is released under CC0 (Public Domain).
"""

from reportlab.lib.pagesizes import A4
from reportlab.lib.units import cm
from reportlab.pdfgen import canvas
from reportlab.lib.colors import (
    red, green, blue, black, white, orange, purple, gray
)
from reportlab.lib.utils import ImageReader
from PIL import Image
import io


def create_test_pdf(filename: str):
    """Create a test PDF with outlines and images."""
    c = canvas.Canvas(filename, pagesize=A4)
    width, height = A4

    # Track bookmarks for outline
    bookmarks = []

    # ========== Page 1: Title Page ==========
    c.setFont("Helvetica-Bold", 24)
    c.drawCentredString(width / 2, height - 5 * cm, "MuPDF Test Document")

    c.setFont("Helvetica", 14)
    c.drawCentredString(width / 2, height - 7 * cm, "A CC0 Public Domain PDF for Testing")
    c.drawCentredString(width / 2, height - 8 * cm, "Contains: Outlines, Images, and Text")

    # Draw a colorful logo (simple geometric shapes as "image")
    draw_logo(c, width / 2 - 3 * cm, height - 15 * cm, 6 * cm, 4 * cm)

    # Embed a raster image (gradient pattern)
    gradient_img = create_sample_image(200, 100, "gradient")
    c.drawImage(image_to_reader(gradient_img), width / 2 - 3 * cm, height - 20 * cm,
                width=6 * cm, height=3 * cm)

    c.setFont("Helvetica", 10)
    c.drawCentredString(width / 2, 3 * cm, "This document is released under CC0 1.0 Universal (Public Domain)")
    c.drawCentredString(width / 2, 2 * cm, "https://creativecommons.org/publicdomain/zero/1.0/")

    # Add bookmark for title page
    key1 = c.bookmarkPage("title")
    bookmarks.append(("Title Page", "title", 0))

    c.showPage()

    # ========== Page 2: Chapter 1 ==========
    c.setFont("Helvetica-Bold", 20)
    c.drawString(2 * cm, height - 3 * cm, "Chapter 1: Introduction")

    key2 = c.bookmarkPage("chapter1")
    bookmarks.append(("Chapter 1: Introduction", "chapter1", 0))

    c.setFont("Helvetica", 12)
    y = height - 5 * cm
    paragraphs = [
        "This is a test PDF document created for testing PDF reading capabilities.",
        "The document contains multiple features commonly found in real-world PDFs:",
        "",
        "• Hierarchical bookmarks (outlines) for navigation",
        "• Embedded images and graphics",
        "• Multiple pages with different content types",
        "• Various text formatting and layouts",
        "",
        "This file is specifically designed to test MuPDF-based PDF processing tools.",
    ]
    for line in paragraphs:
        c.drawString(2 * cm, y, line)
        y -= 0.6 * cm

    # Draw some shapes as "images"
    draw_shapes(c, 2 * cm, y - 5 * cm, 8 * cm, 4 * cm)

    c.showPage()

    # ========== Page 3: Chapter 1.1 ==========
    c.setFont("Helvetica-Bold", 18)
    c.drawString(2 * cm, height - 3 * cm, "1.1 Background")

    key3 = c.bookmarkPage("section1_1")
    bookmarks.append(("1.1 Background", "section1_1", 1))  # nested under chapter 1

    c.setFont("Helvetica", 12)
    y = height - 5 * cm
    text = [
        "PDF (Portable Document Format) was developed by Adobe in the early 1990s.",
        "It has become the de facto standard for document exchange.",
        "",
        "Key features of PDF include:",
        "• Device-independent rendering",
        "• Font embedding",
        "• Vector graphics support",
        "• Document security features",
    ]
    for line in text:
        c.drawString(2 * cm, y, line)
        y -= 0.6 * cm

    c.showPage()

    # ========== Page 4: Chapter 1.2 ==========
    c.setFont("Helvetica-Bold", 18)
    c.drawString(2 * cm, height - 3 * cm, "1.2 MuPDF Library")

    key4 = c.bookmarkPage("section1_2")
    bookmarks.append(("1.2 MuPDF Library", "section1_2", 1))

    c.setFont("Helvetica", 12)
    y = height - 5 * cm
    text = [
        "MuPDF is a lightweight PDF, XPS, and E-book viewer.",
        "It is developed by Artifex Software.",
        "",
        "Features:",
        "• Fast rendering engine",
        "• Small memory footprint",
        "• Support for PDF, XPS, EPUB, and other formats",
        "• Extensive API for document manipulation",
    ]
    for line in text:
        c.drawString(2 * cm, y, line)
        y -= 0.6 * cm

    # Add an image-like graphic
    draw_chart(c, 2 * cm, y - 6 * cm, 10 * cm, 5 * cm)

    c.showPage()

    # ========== Page 5: Chapter 2 ==========
    c.setFont("Helvetica-Bold", 20)
    c.drawString(2 * cm, height - 3 * cm, "Chapter 2: Image Gallery")

    key5 = c.bookmarkPage("chapter2")
    bookmarks.append(("Chapter 2: Image Gallery", "chapter2", 0))

    c.setFont("Helvetica", 12)
    c.drawString(2 * cm, height - 5 * cm, "This page contains various graphical elements:")

    # Embed raster images (different patterns)
    gradient_img = create_sample_image(150, 100, "gradient")
    c.drawImage(image_to_reader(gradient_img), 2 * cm, height - 10 * cm,
                width=5 * cm, height=3 * cm)
    c.drawString(2 * cm, height - 10.5 * cm, "Gradient Image")

    checker_img = create_sample_image(150, 100, "checker")
    c.drawImage(image_to_reader(checker_img), 8 * cm, height - 10 * cm,
                width=5 * cm, height=3 * cm)
    c.drawString(8 * cm, height - 10.5 * cm, "Checkerboard Image")

    circles_img = create_sample_image(150, 100, "circles")
    c.drawImage(image_to_reader(circles_img), 14 * cm, height - 10 * cm,
                width=5 * cm, height=3 * cm)
    c.drawString(14 * cm, height - 10.5 * cm, "Circles Image")

    # Also draw vector graphics for comparison
    draw_logo(c, 2 * cm, height - 17 * cm, 5 * cm, 4 * cm)
    draw_shapes(c, 8 * cm, height - 17 * cm, 5 * cm, 4 * cm)
    draw_chart(c, 2 * cm, height - 25 * cm, 11 * cm, 6 * cm)

    c.showPage()

    # ========== Page 6: Appendix ==========
    c.setFont("Helvetica-Bold", 20)
    c.drawString(2 * cm, height - 3 * cm, "Appendix: License")

    key6 = c.bookmarkPage("appendix")
    bookmarks.append(("Appendix: License", "appendix", 0))

    c.setFont("Helvetica", 11)
    y = height - 5 * cm
    license_text = [
        "CC0 1.0 Universal (CC0 1.0) Public Domain Dedication",
        "",
        "The person who associated a work with this deed has dedicated the work",
        "to the public domain by waiving all of his or her rights to the work",
        "worldwide under copyright law, including all related and neighboring",
        "rights, to the extent allowed by law.",
        "",
        "You can copy, modify, distribute and perform the work, even for",
        "commercial purposes, all without asking permission.",
        "",
        "For more information:",
        "https://creativecommons.org/publicdomain/zero/1.0/",
    ]
    for line in license_text:
        c.drawString(2 * cm, y, line)
        y -= 0.5 * cm

    c.showPage()

    # ========== Create Outline ==========
    # Build hierarchical outline
    outline_items = []
    parent_key = None

    for title, key, level in bookmarks:
        if level == 0:
            c.addOutlineEntry(title, key, level=0)
        else:
            c.addOutlineEntry(title, key, level=1)

    c.save()
    print(f"Created: {filename}")
    print(f"  Pages: 6")
    print(f"  Bookmarks: {len(bookmarks)}")
    print("  Features: outlines, graphics, multi-page")


def draw_logo(c, x, y, w, h):
    """Draw a simple logo with overlapping shapes."""
    # Background rectangle
    c.setFillColor(gray)
    c.rect(x, y, w, h, fill=1, stroke=0)

    # Overlapping circles
    c.setFillColor(red)
    c.circle(x + w * 0.3, y + h * 0.5, h * 0.3, fill=1, stroke=0)

    c.setFillColor(green)
    c.circle(x + w * 0.5, y + h * 0.5, h * 0.3, fill=1, stroke=0)

    c.setFillColor(blue)
    c.circle(x + w * 0.7, y + h * 0.5, h * 0.3, fill=1, stroke=0)

    # Text
    c.setFillColor(white)
    c.setFont("Helvetica-Bold", 10)
    c.drawCentredString(x + w / 2, y + 0.3 * cm, "TEST LOGO")


def draw_shapes(c, x, y, w, h):
    """Draw various geometric shapes."""
    # Rectangle
    c.setFillColor(orange)
    c.rect(x, y + h * 0.6, w * 0.3, h * 0.35, fill=1, stroke=1)

    # Triangle
    c.setFillColor(purple)
    path = c.beginPath()
    path.moveTo(x + w * 0.35 + w * 0.15, y + h * 0.95)
    path.lineTo(x + w * 0.35, y + h * 0.6)
    path.lineTo(x + w * 0.65, y + h * 0.6)
    path.close()
    c.drawPath(path, fill=1, stroke=1)

    # Circle
    c.setFillColor(green)
    c.circle(x + w * 0.85, y + h * 0.77, h * 0.17, fill=1, stroke=1)

    # Pentagon approximation
    c.setFillColor(blue)
    c.rect(x + w * 0.1, y + h * 0.1, w * 0.25, h * 0.35, fill=1, stroke=1)

    # Star-like shape (diamond)
    c.setFillColor(red)
    path = c.beginPath()
    cx, cy = x + w * 0.6, y + h * 0.27
    size = h * 0.2
    path.moveTo(cx, cy + size)
    path.lineTo(cx - size, cy)
    path.lineTo(cx, cy - size)
    path.lineTo(cx + size, cy)
    path.close()
    c.drawPath(path, fill=1, stroke=1)


def draw_chart(c, x, y, w, h):
    """Draw a simple bar chart."""
    c.setStrokeColor(black)
    c.setLineWidth(1)

    # Axes
    c.line(x + 1 * cm, y, x + 1 * cm, y + h - 0.5 * cm)
    c.line(x + 1 * cm, y, x + w - 0.5 * cm, y)

    # Bars
    bar_width = (w - 2 * cm) / 5
    colors = [red, green, blue, orange, purple]
    heights = [0.7, 0.5, 0.9, 0.4, 0.6]
    labels = ["A", "B", "C", "D", "E"]

    for i, (color, h_ratio, label) in enumerate(zip(colors, heights, labels)):
        bar_x = x + 1.2 * cm + i * bar_width
        bar_h = (h - 1 * cm) * h_ratio
        c.setFillColor(color)
        c.rect(bar_x, y + 0.2 * cm, bar_width * 0.8, bar_h, fill=1, stroke=1)

        # Label
        c.setFillColor(black)
        c.setFont("Helvetica", 8)
        c.drawCentredString(bar_x + bar_width * 0.4, y - 0.3 * cm, label)

    # Title
    c.setFont("Helvetica-Bold", 10)
    c.drawCentredString(x + w / 2, y + h - 0.3 * cm, "Sample Chart")


def create_sample_image(width: int, height: int, pattern: str = "gradient") -> Image.Image:
    """Create a sample raster image programmatically."""
    img = Image.new("RGB", (width, height), "white")
    pixels = img.load()

    if pattern == "gradient":
        # Create a colorful gradient
        for y in range(height):
            for x in range(width):
                r = int(255 * x / width)
                g = int(255 * y / height)
                b = int(255 * (1 - x / width))
                pixels[x, y] = (r, g, b)
    elif pattern == "checker":
        # Create a checkerboard pattern
        cell_size = max(width, height) // 8
        for y in range(height):
            for x in range(width):
                if ((x // cell_size) + (y // cell_size)) % 2 == 0:
                    pixels[x, y] = (200, 50, 50)
                else:
                    pixels[x, y] = (50, 50, 200)
    elif pattern == "circles":
        # Create concentric circles
        cx, cy = width // 2, height // 2
        max_r = min(width, height) // 2
        for y in range(height):
            for x in range(width):
                dist = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
                ring = int(dist / (max_r / 5)) % 3
                colors = [(255, 100, 100), (100, 255, 100), (100, 100, 255)]
                pixels[x, y] = colors[ring]

    return img


def image_to_reader(img: Image.Image) -> ImageReader:
    """Convert PIL Image to ReportLab ImageReader."""
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    buf.seek(0)
    return ImageReader(buf)


if __name__ == "__main__":
    create_test_pdf("test-with-outline-and-images.pdf")
