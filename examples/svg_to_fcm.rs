//! Complete SVG to Print-and-Cut FCM conversion example
//!
//! Run with: cargo run --example svg_to_fcm

use fcmlib::{
    svg_path::{SvgConfig, SvgPathParser},
    AlignmentData, CutData, FcmFile, FileHeader, FileType, FileVariant,
    Generator, Path, PathTool, Piece, PieceRestrictions, PieceTable, Point,
    registration_marks::{self, PageSize},
};

pub const INSET_X: f64 = 8.0;
pub const INSET_Y: f64 = 9.0;

use std::env;
use std::fs;
use std::path::Path as FilePath;

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

    // Calculate bounding box for piece dimensions
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;

    for shape in &shapes {
        min_x = min_x.min(shape.start.x);
        min_y = min_y.min(shape.start.y);
        max_x = max_x.max(shape.start.x);
        max_y = max_y.max(shape.start.y);

        for outline in &shape.outlines {
            match outline {
                fcmlib::Outline::Line(segments) => {
                    for seg in segments {
                        min_x = min_x.min(seg.end.x);
                        min_y = min_y.min(seg.end.y);
                        max_x = max_x.max(seg.end.x);
                        max_y = max_y.max(seg.end.y);
                    }
                }
                fcmlib::Outline::Bezier(segments) => {
                    for seg in segments {
                        min_x = min_x.min(seg.end.x).min(seg.control1.x).min(seg.control2.x);
                        min_y = min_y.min(seg.end.y).min(seg.control1.y).min(seg.control2.y);
                        max_x = max_x.max(seg.end.x).max(seg.control1.x).max(seg.control2.x);
                        max_y = max_y.max(seg.end.y).max(seg.control1.y).max(seg.control2.y);
                    }
                }
            }
        }
    }

    let width = (max_x - min_x) as u32;
    let height = (max_y - min_y) as u32;
    let center_x = (min_x + max_x) as f32 / 2.0;
    let center_y = (min_y + max_y) as f32 / 2.0;

    // Create paths from shapes
    let paths: Vec<Path> = shapes
        .into_iter()
        .map(|shape| Path {
            tool: PathTool::TOOL_CUT,
            shape: Some(fcmlib::PathShape {
                start: Point {
                    x: shape.start.x - center_x as i32,
                    y: shape.start.y - center_y as i32,
                },
                outlines: shape.outlines.into_iter().map(|outline| {
                    match outline {
                        fcmlib::Outline::Line(segments) => {
                            fcmlib::Outline::Line(segments.into_iter().map(|seg| {
                                fcmlib::SegmentLine {
                                    end: Point {
                                        x: seg.end.x - center_x as i32,
                                        y: seg.end.y - center_y as i32,
                                    }
                                }
                            }).collect())
                        }
                        fcmlib::Outline::Bezier(segments) => {
                            fcmlib::Outline::Bezier(segments.into_iter().map(|seg| {
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
            rhinestone_diameter: None,
            rhinestones: vec![],
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
        paths,
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
            thumbnail: vec![0; 9],
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
