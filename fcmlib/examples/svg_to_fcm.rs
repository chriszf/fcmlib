//! Complete SVG to Print-and-Cut FCM conversion example
//!
//! Run with: cargo run --example svg_to_fcm

use fcmlib::{
    svg_path::{SvgConfig, SvgPathParser},
    AlignmentData, CutData, FcmFile, FileHeader, FileType, FileVariant,
    Generator, Outline, Path, PathTool, Piece, PieceRestrictions, PieceTable, Point,
    registration_marks::{self, PageSize},
};

// For my printer, top is 9, buttom is 12

pub const INSET_X: f64 = 8.0;
pub const INSET_Y: f64 = 10.0;

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
    0xff, 0xff, 0xff, 0xff, // Palette entry 1: white (BGR + reserved)
];

/// Generate 88x88 monochrome BMP thumbnail from path bounds.
///
/// Must be called with paths in their pre-centering coordinate space (i.e.
/// before the per-piece transform offset is subtracted), and with the
/// matching min/max bounds in that same space.
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
                        // Approximate bezier with line segments to endpoints
                        // (could subdivide for smoother curves)
                        for seg in segs {
                            let (nx, ny) = transform(seg.end.x, seg.end.y);
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

fn find_cut_layer_bounds(svg: &str) -> (usize, usize)
{
    let group_start = svg.find(r#"id="Cut-Layer""#).
        expect("Cut-Layer group not found");

    let tag_start = svg[..group_start].rfind("<").unwrap();

    // Find the end of the <g tag... but whyyy
    let tag_end = svg[group_start..].find('>').unwrap() + group_start + 1;

    let mut depth = 1usize;
    let mut pos = tag_end;

    // Loop through <g> and </g> tags until we find the matching </g> to the cut-layer group 
    let group_end = loop {
        let next_open = svg[pos..].find("<g").map(|i| i + pos);
        let next_close = svg[pos..].find("</g>").map(|i| i + pos);

        match (next_open, next_close) {
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                pos = o + 2;
            }

            (_, Some(c)) => {
                depth -= 1;
                if depth == 0 {
                    break c + 4;
                }

                pos = c + 4;
            }
            _ => panic!("Malformed SVG: Unmatched <g> tag."),
        }
    };

    (tag_start, group_end)
}

fn extract_cut_layer(svg: &str, group_start:usize, group_end:usize) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let content = &svg[group_start..group_end];

    let mut paths = Vec::new();
    let mut search = content;

    while let Some(d_pos) = search.find(r#" d=""#) {
        let rest = &search[d_pos + 4..]; // Skip past the d=... presumably search until the last
                                         // quote
        if let Some(d_end) = rest.find('"') {
            paths.push(rest[..d_end].to_string());
        }
        search = &search[d_pos + 4..];
    }

    Ok(paths)
}


/// Convert SVG path data to a print-and-cut FCM file
fn svg_to_print_and_cut_fcm(
    svg_paths: &Vec<String>,
    page: &PageSize,
    svg_dpi: f64,
) -> Result<FcmFile, Box<dyn std::error::Error>> {
    // Parse SVG path
    let config = SvgConfig {
        dpi: svg_dpi,
        scale: 1.0,
        offset_x_mm: 0.0,
        offset_y_mm: 0.0,
    };

    let parser = SvgPathParser::new(config);
    let mut shapes = Vec::new();
    for path in svg_paths {
        shapes.extend(parser.parse(path)?);
    }

    // Build paths in their original (uncentered) coordinate space first.
    // We need them in this space to generate a thumbnail that matches the
    // bounds we compute below; centering happens afterwards.
    let paths: Vec<Path> = shapes
        .into_iter()
        .map(|shape| Path {
            tool: PathTool::TOOL_CUT,
            shape: Some(shape),
            rhinestone_diameter: None,
            rhinestones: vec![],
        })
        .collect();

    // Calculate bounding box across every point of every path.
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

    // Generate the thumbnail using the uncentered paths and bounds. This
    // has to happen BEFORE we recenter, because the bounds we computed are
    // in the same space as the paths.
    let thumbnail = generate_thumbnail(min_x, min_y, max_x, max_y, &paths);

    // Now recenter each path relative to the piece center for the FCM piece.
    let centered_paths: Vec<Path> = paths
        .into_iter()
        .map(|path| {
            let shape = path.shape.expect("path built above always has a shape");
            Path {
                tool: path.tool,
                shape: Some(fcmlib::PathShape {
                    start: Point {
                        x: shape.start.x - center_x as i32,
                        y: shape.start.y - center_y as i32,
                    },
                    outlines: shape.outlines.into_iter().map(|outline| {
                        match outline {
                            Outline::Line(segments) => {
                                Outline::Line(segments.into_iter().map(|seg| {
                                    fcmlib::SegmentLine {
                                        end: Point {
                                            x: seg.end.x - center_x as i32,
                                            y: seg.end.y - center_y as i32,
                                        }
                                    }
                                }).collect())
                            }
                            Outline::Bezier(segments) => {
                                Outline::Bezier(segments.into_iter().map(|seg| {
                                    fcmlib::SegmentBezier {
                                        control1: Point {
                                            x: seg.control1.x - center_x as i32,
                                            y: seg.control1.y - center_y as i32,
                                        },
                                        control2: Point {
                                            x: seg.control2.x - center_x as i32,
                                            y: seg.control2.y - center_y as i32,
                                        },
                                        end: Point {
                                            x: seg.end.x - center_x as i32,
                                            y: seg.end.y - center_y as i32,
                                        },
                                    }
                                }).collect())
                            }
                        }
                    }).collect(),
                }),
                rhinestone_diameter: path.rhinestone_diameter,
                rhinestones: path.rhinestones,
            }
        })
        .collect();

    // Create piece
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

    // Page dimensions in FCM units
    let page_width = (page.width_mm * 100.0) as u32;
    let page_height = (page.height_mm * 100.0) as u32;

    // Create FCM file
    Ok(FcmFile {
        file_header: FileHeader {
            variant: FileVariant::VCM,
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
                needed: true,
                marks: registration_marks::get_fcm_alignment_marks_inset(page, INSET_X, INSET_Y),
            }),
        },
        piece_table: PieceTable {
            pieces: vec![(0, piece)],
        },
    })
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

    let svg_content = fs::read_to_string(input_path)?;
    
    let page_size = &PageSize::LETTER_LANDSCAPE;
    let dpi = 300.0;

    // Extract paths from Cut-Layer somehow...???.
    let (group_start, group_end) = find_cut_layer_bounds(&svg_content);
    let paths = extract_cut_layer(&svg_content, group_start, group_end)?;

    let print_svg = format!("{}{}{}", &svg_content[..group_start],
            registration_marks::generate_embeddable_marks_inset_svg(page_size, Some(dpi), INSET_X, INSET_Y),
            &svg_content[group_end..]);

    // Extract everything but the cut layer.

    println!("=== SVG to Print-and-Cut FCM Converter ===\n");
    println!("Converting all paths...");
    let output = format!("{}.fcm", input_stem);
    match svg_to_print_and_cut_fcm(&paths, page_size, dpi) {
        Ok(fcm) => {
            fcm.to_file(&output).expect("Failed to write FCM");
            println!("  Created: {}", output);
            println!("  Page: {}mm x {}mm",
                fcm.cut_data.cut_width as f64 / 100.0,
                fcm.cut_data.cut_height as f64 / 100.0);
            if let Some(align) = &fcm.cut_data.alignment {
                println!("  Registration marks: {}", align.marks.len());
            }
        }
        Err(e) => println!("  Error: {}", e),
    }

    let print_path = format!("{}_print.svg", input_stem);
    fs::write(&print_path, &print_svg)?;

    println!("\nFiles created can be loaded on a Brother ScanNCut.");
    println!("Remember to print the artwork with registration marks first!");

    Ok(())
}
