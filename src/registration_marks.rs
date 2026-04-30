//! Registration Mark Generation for Brother ScanNCut Print-and-Cut
//!
//! This module provides utilities for generating registration marks
//! both as SVG (for printing) and as FCM alignment data (for cutting).
//!
//! # Registration Mark Design
//!
//! Brother ScanNCut uses a bullseye pattern with a horizontal bar:
//!
//! ```text
//!      ┌─────────────────┐
//!      │  ████████████   │  ← Horizontal bar (10mm × 1mm)
//!      │     ╭─────╮     │
//!      │    ╱       ╲    │  ← Outer ring (r=5mm, stroke=1mm)
//!      │   │  ╭───╮  │   │
//!      │   │ │  ●  │ │   │  ← Inner ring (r=3mm) + center dot (r=1mm)
//!      │   │  ╰───╯  │   │
//!      │    ╲       ╱    │
//!      │     ╰─────╯     │
//!      │                 │
//!      └─────────────────┘
//!          White background (16mm × 18mm)
//! ```
//!
//! # Layout Rules
//!
//! - 4 marks positioned at corners of the printable area
//! - X inset: 12.0mm from left/right edges
//! - Y inset: ~14.0mm from top/bottom edges (13.98mm exactly)
//! - Marks are centered at these inset positions
//!
//! After experimentation, inset does not seem to actually matter,
//! the machine scales to match the registration marks.

use crate::Point;

/// Registration mark dimensions (all values in mm, extracted from Illustrator)
pub mod dimensions {
    /// White background rectangle width (45.4pt = 16.0mm)
    pub const BG_WIDTH_MM: f64 = 16.0;
    /// White background rectangle height (51pt = 18.0mm)
    pub const BG_HEIGHT_MM: f64 = 18.0;

    /// Center dot radius (2.8pt = 0.99mm)
    pub const CENTER_DOT_RADIUS_MM: f64 = 0.99;
    /// Inner ring outer radius (8.5pt = 3.0mm)
    pub const INNER_RING_OUTER_MM: f64 = 3.0;
    /// Inner ring inner radius (5.7pt = 2.0mm)
    pub const INNER_RING_INNER_MM: f64 = 2.0;
    /// Outer ring outer radius (14.2pt = 5.0mm)
    pub const OUTER_RING_OUTER_MM: f64 = 5.0;
    /// Outer ring inner radius (11.3pt = 4.0mm)
    pub const OUTER_RING_INNER_MM: f64 = 4.0;

    /// Horizontal bar width (28.3pt = 10.0mm)
    pub const BAR_WIDTH_MM: f64 = 10.0;
    /// Horizontal bar height (2.8pt = 1.0mm)
    pub const BAR_HEIGHT_MM: f64 = 1.0;

    /// Standard X inset from page edge to mark center
    pub const X_INSET_MM: f64 = 12.0;
    /// Standard Y inset from page edge to mark center
    pub const Y_INSET_MM: f64 = 13.98;
}

/// Page size definition
#[derive(Debug, Clone, Copy)]
pub struct PageSize {
    pub width_mm: f64,
    pub height_mm: f64,
}

impl PageSize {
    /// US Letter: 8.5" × 11"
    pub const LETTER: PageSize = PageSize { width_mm: 215.9, height_mm: 279.4 };
    pub const LETTER_LANDSCAPE: PageSize = PageSize { width_mm: 279.4, height_mm: 215.9 };
    /// ISO A4: 210mm × 297mm
    pub const A4: PageSize = PageSize { width_mm: 210.0, height_mm: 297.0 };
    pub const A4_LANDSCAPE: PageSize = PageSize { width_mm: 297.0, height_mm: 210.0 };
    /// 12" × 12" craft mat
    pub const SQUARE_12: PageSize = PageSize { width_mm: 304.8, height_mm: 304.8 };
    /// 12" × 24" craft mat
    pub const LONG_12X24: PageSize = PageSize { width_mm: 304.8, height_mm: 609.6 };

    /// Create custom page size
    pub fn new(width_mm: f64, height_mm: f64) -> Self {
        Self { width_mm, height_mm }
    }

    /// Get page dimensions in FCM units (hundredths of mm)
    pub fn to_fcm_units(&self) -> (u32, u32) {
        ((self.width_mm * 100.0) as u32, (self.height_mm * 100.0) as u32)
    }
}

/// Position of a single registration mark
#[derive(Debug, Clone, Copy)]
pub struct MarkPosition {
    /// X coordinate of mark center in mm
    pub x_mm: f64,
    /// Y coordinate of mark center in mm
    pub y_mm: f64,
}

impl MarkPosition {
    /// Convert to FCM Point (hundredths of mm)
    pub fn to_fcm_point(&self) -> Point {
        Point {
            x: (self.x_mm * 100.0).round() as i32,
            y: (self.y_mm * 100.0).round() as i32,
        }
    }

    /// Convert to SVG coordinates at given DPI
    pub fn to_svg_coords(&self, dpi: f64) -> (f64, f64) {
        let mm_to_px = dpi / 25.4;
        (self.x_mm * mm_to_px, self.y_mm * mm_to_px)
    }
}

pub fn calculate_mark_positions_with_inset(page: &PageSize, x_inset_mm: f64, y_inset_mm: f64) -> [MarkPosition; 4] {
    [
        // Top-left
        MarkPosition {
            x_mm: x_inset_mm,
            y_mm: y_inset_mm,
        },
        // Top-right
        MarkPosition {
            x_mm: page.width_mm - x_inset_mm,
            y_mm: y_inset_mm,
        },
        // Bottom-right
        MarkPosition {
            x_mm: page.width_mm - x_inset_mm,
            y_mm: page.height_mm - y_inset_mm,
        },
        // Bottom-left
        MarkPosition {
            x_mm: x_inset_mm,
            y_mm: page.height_mm - y_inset_mm,
        },
    ]
}

/// Calculate the 4 registration mark positions for a given page size
pub fn calculate_mark_positions(page: &PageSize) -> [MarkPosition; 4] {
    use dimensions::*;
    calculate_mark_positions_with_inset(page, X_INSET_MM, Y_INSET_MM)
}

/// Get FCM AlignmentData marks for a page size
pub fn get_fcm_alignment_marks(page: &PageSize) -> Vec<Point> {
    calculate_mark_positions(page)
        .iter()
        .map(|pos| pos.to_fcm_point())
        .collect()
}

/// Get FCM AlignmentData marks for a page size
pub fn get_fcm_alignment_marks_inset(page: &PageSize, x_inset: f64, y_inset: f64) -> Vec<Point> {
    calculate_mark_positions_with_inset(page, x_inset, y_inset)
        .iter()
        .map(|pos| pos.to_fcm_point())
        .collect()
}

/// Exact registration mark SVG from Adobe Illustrator ScanNCut plugin.
/// This is R1 verbatim - centered at (34.03858, 39.63858) in the original.
const MARK_TEMPLATE: &str = r##"<rect x="11.33858" y="11.33858" width="45.4" height="51" fill="#fff"/>
      <circle cx="34.03858" cy="39.63858" r="2.8" fill="#070404"/>
      <path d="M34.03858,48.13858c-4.7,0-8.5-3.8-8.5-8.5s3.8-8.5,8.5-8.5,8.5,3.8,8.5,8.5-3.8,8.5-8.5,8.5ZM34.03858,34.03858c-3.1,0-5.7,2.5-5.7,5.7,0,3.1,2.5,5.7,5.7,5.7s5.7-2.5,5.7-5.7c-.1-3.2-2.6-5.7-5.7-5.7Z" fill="#070404"/>
      <path d="M34.03858,53.83858c-7.8,0-14.2-6.4-14.2-14.2s6.4-14.2,14.2-14.2,14.2,6.4,14.2,14.2c0,7.9-6.4,14.2-14.2,14.2ZM34.03858,28.33858c-6.3,0-11.3,5.1-11.3,11.3s5.1,11.3,11.3,11.3,11.3-5,11.3-11.3-5.1-11.3-11.3-11.3Z" fill="#070404"/>
      <rect x="19.83858" y="19.83858" width="28.3" height="2.8" fill="#070404"/>"##;

/// The center point of the template mark (where it was in the original Illustrator export)
const TEMPLATE_CENTER_X: f64 = 34.03858;
const TEMPLATE_CENTER_Y: f64 = 39.63858;

/// Generate a single registration mark as SVG at the given center position.
/// Uses the exact mark from Adobe Illustrator, positioned via transform.
pub fn generate_mark_svg(cx: f64, cy: f64, id: &str, dpi: f64) -> String {
    // The mark svg is written in 72dpi probably, we need to rescale it possibly
    let dpi_scale = dpi / 72.0;

    format!(
        "  <g id=\"{}\" transform=\"translate({:.5}, {:.5}) scale({:.5}) translate({:.5}, {:.5})\">\n      {}\n  </g>",
        id, cx, cy, dpi_scale, -TEMPLATE_CENTER_X, -TEMPLATE_CENTER_Y, MARK_TEMPLATE
    )
}


pub fn generate_embeddable_marks_inset_svg(page: &PageSize, dpi: Option<f64>, x_inset_mm: f64, y_inset_mm: f64) -> String {
    let positions = calculate_mark_positions_with_inset(page, x_inset_mm, y_inset_mm);
    let dpi = dpi.unwrap_or(300.0);

    let mut marks_svg = String::new();
    marks_svg.push_str(" <g id=\"registration_marks\">\n");
    for (i, pos) in positions.iter().enumerate() {
        let (x,y) = pos.to_svg_coords(dpi);
        marks_svg.push_str(&generate_mark_svg(x,y, &format!("R{}", i+1), dpi));
        marks_svg.push('\n');
    }

    marks_svg.push_str(" </g>\n");
    marks_svg
}
pub fn generate_embeddable_marks_svg(page: &PageSize, dpi: Option<f64>) -> String {
    use dimensions::*;
    generate_embeddable_marks_inset_svg(page, dpi, X_INSET_MM, Y_INSET_MM) 
}

/// Generate complete SVG with all 4 registration marks for a page
pub fn generate_registration_marks_svg(page: &PageSize, dpi: Option<f64>) -> String {
    let mm_to_pt = 72.0 / 25.4;
    let width_pt = page.width_mm * mm_to_pt;
    let height_pt = page.height_mm * mm_to_pt;

    let positions = calculate_mark_positions(page);
    let dpi = dpi.unwrap_or(300.0);

    let marks: Vec<String> = positions
        .iter()
        .enumerate()
        .map(|(i, pos)| {
            let (x, y) = pos.to_svg_coords(dpi);
            generate_mark_svg(x, y, &format!("R{}", i + 1), dpi)
        })
        .collect();

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg"
     width="{width_mm}mm" height="{height_mm}mm"
     viewBox="0 0 {width_pt:.0} {height_pt:.0}">
  <title>Brother ScanNCut Registration Marks - {width_mm}mm × {height_mm}mm</title>
  <g id="registration_marks">
{marks}
  </g>
</svg>"#,
        width_mm = page.width_mm,
        height_mm = page.height_mm,
        marks = marks.join("\n")
    )
}

/// Generate a single registration mark as standalone SVG (for embedding)
pub fn generate_single_mark_svg() -> String {
    use dimensions::*;

    let mm_to_pt = 72.0 / 25.4;
    let size = BG_HEIGHT_MM * mm_to_pt;
    let center = size / 2.0;

    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size:.0}" height="{size:.0}" viewBox="0 0 {size:.0} {size:.0}">
{mark}
</svg>"#,
        mark = generate_mark_svg(center, center, "mark", 72.0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_letter_positions() {
        let positions = calculate_mark_positions(&PageSize::LETTER);

        // Top-left
        assert!((positions[0].x_mm - 12.0).abs() < 0.01);
        assert!((positions[0].y_mm - 13.98).abs() < 0.01);

        // Top-right
        assert!((positions[1].x_mm - 203.9).abs() < 0.01);

        // Bottom-right
        assert!((positions[2].y_mm - 265.42).abs() < 0.01);
    }

    #[test]
    fn test_fcm_points() {
        let marks = get_fcm_alignment_marks(&PageSize::LETTER);
        assert_eq!(marks.len(), 4);
        assert_eq!(marks[0].x, 1200); // 12.00mm
        assert_eq!(marks[0].y, 1398); // 13.98mm
    }
}
