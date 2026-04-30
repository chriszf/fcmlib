//! Generate a 5x7" sticker sheet with registration marks + FCM cut file
//!
//! Usage: cargo run --example sticker_sheet -- <input.svg>
//!
//! Outputs:
//!   - <input>_print.svg  (artwork + registration marks for printing)
//!   - <input>.fcm        (cut paths for ScanNCut)

use fcmlib::{
    registration_marks::{self, PageSize},
    AlignmentData, CutData, FcmFile, FileHeader, FileType, FileVariant,
    Generator, Outline, Path, PathTool, Piece, PieceRestrictions,
    PieceTable,
};
use std::env;
use std::fs;
use std::path::Path as FilePath;

/// BMP header for 88x88 monochrome image (62 bytes)
const BMP_HEADER: &[u8] = &[
    0x42, 0x4d,             // "BM"
    0x5e, 0x04, 0x00, 0x00, // File size: 1118 bytes
    0x00, 0x00, 0x00, 0x00, // Reserved
    0x3e, 0x00, 0x00, 0x00, // Pixel data offset: 62 bytes
    0x28, 0x00, 0x00, 0x00, // DIB header size: 40 bytes
    0x58, 0x00, 0x00, 0x00, // Width: 88 pixels
    0x58, 0x00, 0x00, 0x00, // Height: 88 pixels
    0x01, 0x00,             // Color planes: 1
    0x01, 0x00,             // Bits per pixel: 1
    0x00, 0x00, 0x00, 0x00, // Compression: none
    0x00, 0x00, 0x00, 0x00, // Image size (can be 0 for uncompressed)
    0xc4, 0x0e, 0x00, 0x00, // Horizontal resolution
    0xc4, 0x0e, 0x00, 0x00, // Vertical resolution
    0x02, 0x00, 0x00, 0x00, // Colors in palette: 2
    0x02, 0x00, 0x00, 0x00, // Important colors: 2
    0x00, 0x00, 0x00, 0xff, // Palette entry 0: black (BGR + reserved)
    0xff, 0xff, 0xff, 0xff, // Palette entry 1: white (BGR + reserved) - note: last byte is 0xff not 0x00 per original
];

/// Generate 88x88 monochrome BMP thumbnail from path bounds
fn generate_thumbnail(min_x: i32, min_y: i32, max_x: i32, max_y: i32, paths: &[Path]) -> Vec<u8> {
    const SIZE: usize = 88;
    const ROW_BYTES: usize = 12; // 88 bits = 11 bytes, padded to 12

    // Start with white image (all 1s = white in 1-bit BMP)
    let mut pixels = vec![0xFFu8; SIZE * ROW_BYTES];

    let width = (max_x - min_x) as f64;
    let height = (max_y - min_y) as f64;

    if width <= 0.0 || height <= 0.0 {
        // Return blank thumbnail
        let mut bmp = BMP_HEADER.to_vec();
        bmp.extend_from_slice(&pixels);
        return bmp;
    }

    // Scale to fit in 80x80 (leaving 4px margin)
    let scale = 80.0 / width.max(height);
    let offset_x = (SIZE as f64 - width * scale) / 2.0;
    let offset_y = (SIZE as f64 - height * scale) / 2.0;

    // Helper to set a pixel (black)
    let set_pixel = |pixels: &mut [u8], x: i32, y: i32| {
        if x >= 0 && x < SIZE as i32 && y >= 0 && y < SIZE as i32 {
            // BMP is bottom-up, so flip y
            let row = SIZE - 1 - y as usize;
            let col = x as usize;
            let byte_idx = row * ROW_BYTES + col / 8;
            let bit_idx = 7 - (col % 8);
            pixels[byte_idx] &= !(1 << bit_idx); // Clear bit = black
        }
    };

    // Draw line using Bresenham's algorithm
    let draw_line = |pixels: &mut [u8], x0: i32, y0: i32, x1: i32, y1: i32| {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut x = x0;
        let mut y = y0;

        loop {
            set_pixel(pixels, x, y);
            if x == x1 && y == y1 { break; }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    };

    // Transform FCM coords to thumbnail coords
    let transform = |px: i32, py: i32| -> (i32, i32) {
        let x = ((px - min_x) as f64 * scale + offset_x) as i32;
        let y = ((py - min_y) as f64 * scale + offset_y) as i32;
        (x, y)
    };

    // Draw all paths
    for path in paths {
        if let Some(shape) = &path.shape {
            let (mut cur_x, mut cur_y) = transform(shape.start.x, shape.start.y);

            for outline in &shape.outlines {
                match outline {
                    Outline::Line(segs) => {
                        for seg in segs {
                            let (nx, ny) = transform(seg.end.x, seg.end.y);
                            draw_line(&mut pixels, cur_x, cur_y, nx, ny);
                            cur_x = nx;
                            cur_y = ny;
                        }
                    }
                    Outline::Bezier(segs) => {
                        // Approximate bezier with line segments
                        for seg in segs {
                            let (nx, ny) = transform(seg.end.x, seg.end.y);
                            // Simple: just draw line to end (could add more points for smoother curves)
                            draw_line(&mut pixels, cur_x, cur_y, nx, ny);
                            cur_x = nx;
                            cur_y = ny;
                        }
                    }
                }
            }
        }
    }

    // Combine header and pixels
    let mut bmp = BMP_HEADER.to_vec();
    bmp.extend_from_slice(&pixels);
    bmp
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <input.svg>", args[0]);
        std::process::exit(1);
    }

    let input_path = &args[1];
    let input_stem = FilePath::new(input_path)
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap();

    // 5x7 inches in mm
    let page = PageSize::new(127.0, 177.8); // 5" x 7"

    println!("Processing: {}", input_path);
    println!("Page size: 5\" x 7\" ({:.1}mm x {:.1}mm)", page.width_mm, page.height_mm);

    // Read the input SVG
    let svg_content = fs::read_to_string(input_path)?;

    // Extract the outermost path from the SVG
    let outermost_path = extract_outermost_path(&svg_content)?;
    println!("Found outermost path with {} characters", outermost_path.len());

    // Parse the viewBox to understand the SVG coordinate system
    let (svg_width, svg_height, viewbox) = parse_svg_dimensions(&svg_content)?;
    println!("SVG dimensions: {}x{}, viewBox: {:?}", svg_width, svg_height, viewbox);

    // --- Generate the printable SVG (artwork + registration marks) ---
    let print_svg = generate_print_svg(&svg_content, &page)?;
    let print_path = format!("{}_print.svg", input_stem);
    fs::write(&print_path, &print_svg)?;
    println!("Created: {}", print_path);

    // --- Generate the FCM file (cut paths only) ---
    let fcm = generate_fcm(&outermost_path, &page, &viewbox)?;
    let fcm_path = format!("{}.fcm", input_stem);
    fcm.to_file(&fcm_path).map_err(|e| format!("FCM write error: {}", e))?;
    println!("Created: {}", fcm_path);

    println!("\nDone! Print {} then load {} on your ScanNCut.", print_path, fcm_path);

    Ok(())
}

/// Extract the outermost/first path from an SVG
fn extract_outermost_path(svg: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Find the first <path d="..."> element
    // This is a simple approach - a real implementation would use an XML parser

    // Look for path elements
    let path_start = svg.find("<path")
        .ok_or("No <path> element found in SVG")?;

    let d_start = svg[path_start..].find("d=\"")
        .ok_or("No 'd' attribute found in path")?;

    let d_content_start = path_start + d_start + 3; // skip 'd="'
    let d_content_end = svg[d_content_start..].find("\"")
        .ok_or("Unclosed 'd' attribute")?;

    Ok(svg[d_content_start..d_content_start + d_content_end].to_string())
}

/// Parse SVG dimensions and viewBox
fn parse_svg_dimensions(svg: &str) -> Result<(f64, f64, (f64, f64, f64, f64)), Box<dyn std::error::Error>> {
    // Extract width
    let width = extract_dimension(svg, "width").unwrap_or(100.0);
    let height = extract_dimension(svg, "height").unwrap_or(100.0);

    // Extract viewBox
    let viewbox = if let Some(vb_start) = svg.find("viewBox=\"") {
        let vb_content_start = vb_start + 9;
        let vb_content_end = svg[vb_content_start..].find("\"").unwrap_or(0);
        let vb_str = &svg[vb_content_start..vb_content_start + vb_content_end];

        let parts: Vec<f64> = vb_str
            .split_whitespace()
            .flat_map(|s| s.split(','))
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        if parts.len() >= 4 {
            (parts[0], parts[1], parts[2], parts[3])
        } else {
            (0.0, 0.0, width, height)
        }
    } else {
        (0.0, 0.0, width, height)
    };

    Ok((width, height, viewbox))
}

fn extract_dimension(svg: &str, attr: &str) -> Option<f64> {
    let pattern = format!("{}=\"", attr);
    let start = svg.find(&pattern)? + pattern.len();
    let end = svg[start..].find("\"")?;
    let value_str = &svg[start..start + end];

    // Strip units (px, mm, in, etc.)
    let numeric: String = value_str.chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();

    numeric.parse().ok()
}

/// Generate a printable SVG with artwork + registration marks
fn generate_print_svg(original_svg: &str, page: &PageSize) -> Result<String, Box<dyn std::error::Error>> {
    let mm_to_pt = 72.0 / 25.4;
    let page_width_pt = page.width_mm * mm_to_pt;
    let page_height_pt = page.height_mm * mm_to_pt;

    // Get registration marks SVG content (just the <g> element, not full SVG)
    let marks = registration_marks::calculate_mark_positions(page);
    let mut marks_svg = String::new();
    marks_svg.push_str("  <g id=\"registration_marks\">\n");
    for (i, mark) in marks.iter().enumerate() {
        let (x, y) = mark.to_svg_coords(72.0);
        marks_svg.push_str(&registration_marks::generate_mark_svg(x, y, &format!("R{}", i + 1), 72.0));
        marks_svg.push('\n');
    }
    marks_svg.push_str("  </g>\n");

    // Extract the inner content of the original SVG (everything between <svg...> and </svg>)
    // Find the opening <svg tag first, then find its closing >
    let svg_tag_start = original_svg.find("<svg").unwrap_or(0);
    let svg_tag_end = original_svg[svg_tag_start..].find(">").map(|i| svg_tag_start + i + 1).unwrap_or(0);
    let svg_close_start = original_svg.rfind("</svg>").unwrap_or(original_svg.len());
    let original_content = &original_svg[svg_tag_end..svg_close_start];

    // The original SVG content is now just the inner elements (paths, groups, etc.)
    // No need to convert <svg> to <g> since we extracted only the inner content
    let original_content = original_content.to_string();

    // Parse original viewBox to scale content
    let (_, _, viewbox) = parse_svg_dimensions(original_svg)?;

    // Calculate scale to fit artwork within the registration mark area
    // Leave margins for the marks (about 25mm on each side)
    let margin_mm = 25.0;
    let available_width = page.width_mm - (margin_mm * 2.0);
    let available_height = page.height_mm - (margin_mm * 2.0);

    let scale_x = (available_width * mm_to_pt) / viewbox.2;
    let scale_y = (available_height * mm_to_pt) / viewbox.3;
    let scale = scale_x.min(scale_y);

    // Center the artwork
    let artwork_width = viewbox.2 * scale;
    let artwork_height = viewbox.3 * scale;
    let offset_x = (page_width_pt - artwork_width) / 2.0;
    let offset_y = (page_height_pt - artwork_height) / 2.0;

    // Build the composite SVG
    let output = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg"
     width="{width_mm}mm" height="{height_mm}mm"
     viewBox="0 0 {width_pt:.0} {height_pt:.0}">
  <title>Sticker Sheet - 5x7 inch with Registration Marks</title>

  <!-- Original artwork, scaled and centered -->
  <g id="artwork" transform="translate({offset_x:.2}, {offset_y:.2}) scale({scale:.4})">
{content}
  </g>

  <!-- Registration marks for print-and-cut -->
{marks}
</svg>"#,
        width_mm = page.width_mm,
        height_mm = page.height_mm,
        width_pt = page_width_pt,
        height_pt = page_height_pt,
        offset_x = offset_x,
        offset_y = offset_y,
        scale = scale,
        content = original_content,
        marks = marks_svg
    );

    Ok(output)
}

/// Generate FCM file with cut paths
fn generate_fcm(
    path_d: &str,
    page: &PageSize,
    viewbox: &(f64, f64, f64, f64),
) -> Result<FcmFile, Box<dyn std::error::Error>> {
    use fcmlib::svg_path::{SvgConfig, SvgPathParser};

    let mm_to_pt = 72.0 / 25.4;
    let page_width_pt = page.width_mm * mm_to_pt;
    let page_height_pt = page.height_mm * mm_to_pt;

    // Calculate the same scaling as the print version
    let margin_mm = 25.0;
    let available_width = page.width_mm - (margin_mm * 2.0);
    let available_height = page.height_mm - (margin_mm * 2.0);

    let scale_x = (available_width * mm_to_pt) / viewbox.2;
    let scale_y = (available_height * mm_to_pt) / viewbox.3;
    let scale = scale_x.min(scale_y);

    let artwork_width = viewbox.2 * scale;
    let artwork_height = viewbox.3 * scale;
    let offset_x_pt = (page_width_pt - artwork_width) / 2.0;
    let offset_y_pt = (page_height_pt - artwork_height) / 2.0;

    // Convert offsets to mm
    let offset_x_mm = offset_x_pt / mm_to_pt;
    let offset_y_mm = offset_y_pt / mm_to_pt;

    // Parse the SVG path with proper scaling
    // The SVG path is in viewBox units, we need to scale it
    let config = SvgConfig {
        dpi: 72.0 / scale, // Adjust DPI to account for scaling
        scale: 1.0,
        offset_x_mm,
        offset_y_mm,
    };

    let parser = SvgPathParser::new(config);
    let shapes = parser.parse(path_d)?;

    if shapes.is_empty() {
        return Err("No paths found in SVG".into());
    }

    // Build paths from shapes
    let paths: Vec<Path> = shapes
        .into_iter()
        .map(|shape| Path {
            tool: PathTool::TOOL_CUT,  // Cut only, no draw for print-and-cut
            shape: Some(shape),
            rhinestone_diameter: None,
            rhinestones: vec![],
        })
        .collect();

    // Calculate piece bounds
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;

    for path in &paths {
        if let Some(shape) = &path.shape {
            min_x = min_x.min(shape.start.x);
            min_y = min_y.min(shape.start.y);
            max_x = max_x.max(shape.start.x);
            max_y = max_y.max(shape.start.y);

            for outline in &shape.outlines {
                match outline {
                    Outline::Line(segs) => {
                        for s in segs {
                            min_x = min_x.min(s.end.x);
                            min_y = min_y.min(s.end.y);
                            max_x = max_x.max(s.end.x);
                            max_y = max_y.max(s.end.y);
                        }
                    }
                    Outline::Bezier(segs) => {
                        for s in segs {
                            min_x = min_x.min(s.end.x).min(s.control1.x).min(s.control2.x);
                            min_y = min_y.min(s.end.y).min(s.control1.y).min(s.control2.y);
                            max_x = max_x.max(s.end.x).max(s.control1.x).max(s.control2.x);
                            max_y = max_y.max(s.end.y).max(s.control1.y).max(s.control2.y);
                        }
                    }
                }
            }
        }
    }

    let width = (max_x - min_x) as u32;
    let height = (max_y - min_y) as u32;
    let center_x = (min_x + max_x) as f32 / 2.0;
    let center_y = (min_y + max_y) as f32 / 2.0;

    // Generate thumbnail before transforming paths
    let thumbnail = generate_thumbnail(min_x, min_y, max_x, max_y, &paths);

    // Recenter paths relative to piece center
    let centered_paths: Vec<Path> = paths
        .into_iter()
        .map(|mut path| {
            if let Some(ref mut shape) = path.shape {
                shape.start.x -= center_x as i32;
                shape.start.y -= center_y as i32;

                for outline in &mut shape.outlines {
                    match outline {
                        Outline::Line(segs) => {
                            for s in segs {
                                s.end.x -= center_x as i32;
                                s.end.y -= center_y as i32;
                            }
                        }
                        Outline::Bezier(segs) => {
                            for s in segs {
                                s.control1.x -= center_x as i32;
                                s.control1.y -= center_y as i32;
                                s.control2.x -= center_x as i32;
                                s.control2.y -= center_y as i32;
                                s.end.x -= center_x as i32;
                                s.end.y -= center_y as i32;
                            }
                        }
                    }
                }
            }
            path
        })
        .collect();

    let piece = Piece {
        width,
        height,
        transform: Some((1.0, 0.0, 0.0, 1.0, center_x, center_y)),
        expansion_limit_value: 0,
        reduction_limit_value: 0,
        restriction_flags: PieceRestrictions::empty(),
        label: String::new(),
        paths: centered_paths,
    };

    // Page dimensions in FCM units (hundredths of mm)
    let (page_width, page_height) = page.to_fcm_units();

    // Get registration mark positions
    let marks = registration_marks::get_fcm_alignment_marks(page);

    Ok(FcmFile {
        file_header: FileHeader {
            variant: FileVariant::VCM,  // VCM for print-and-cut
            version: String::from("0100"),
            content_id: 400000002,
            short_name: String::new(),
            long_name: String::from(" "),
            author_name: String::from(" "),
            copyright: String::new(),
            thumbnail_block_size_width: 3,
            thumbnail_block_size_height: 3,
            thumbnail,
            generator: Generator::App(1),
            print_to_cut: Some(true),
        },
        cut_data: CutData {
            file_type: FileType::PrintAndCut,
            mat_id: 0,
            cut_width: page_width,
            cut_height: page_height,
            seam_allowance_width: 0,
            alignment: Some(AlignmentData {
                needed: true,  // Tell the machine to scan for marks
                marks,         // The 4 corner positions
            }),
        },
        piece_table: PieceTable {
            pieces: vec![(0, piece)],
        },
    })
}
