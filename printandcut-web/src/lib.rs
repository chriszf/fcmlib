//! fcm-web/src/lib.rs
//!
//! WASM entry point for the FCM converter.
//! All conversion logic is self-contained here; the only external dependency
//! is fcmlib itself.

use wasm_bindgen::prelude::*;

use fcmlib::{
    svg_path::{SvgConfig, SvgPathParser},
    AlignmentData, CutData, FcmFile, FileHeader, FileType, FileVariant,
    Generator, Path, PathTool, Piece, PieceRestrictions, PieceTable, Point,
    registration_marks::{self, PageSize},
};

const INSET_X: f64 = 8.0;
const INSET_Y: f64 = 9.0;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// A simple string-backed error so we can use ? throughout and surface clean
/// messages to JS via JsError.
#[derive(Debug)]
struct ConvertError(String);

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ConvertError {}

impl From<&str> for ConvertError {
    fn from(s: &str) -> Self { ConvertError(s.to_string()) }
}

impl From<String> for ConvertError {
    fn from(s: String) -> Self { ConvertError(s) }
}

// Allow ? on any Box<dyn Error> result (e.g. from fcmlib's parser)
impl From<Box<dyn std::error::Error>> for ConvertError {
    fn from(e: Box<dyn std::error::Error>) -> Self { ConvertError(e.to_string()) }
}

type Result<T> = std::result::Result<T, ConvertError>;

// ---------------------------------------------------------------------------
// WASM initialisation
// ---------------------------------------------------------------------------

#[wasm_bindgen(start)]
pub fn init() {
    // Routes Rust panics to the browser console — makes debugging much easier.
    console_error_panic_hook::set_once();
}

// ---------------------------------------------------------------------------
// Public WASM entry point
// ---------------------------------------------------------------------------

/// Convert an SVG string into a print-and-cut FCM file.
///
/// Returns a two-element JS Array:
///   [0] — Uint8Array: the .fcm binary
///   [1] — String:     the _print.svg (original SVG with Cut-Layer replaced
///                     by registration marks)
///
/// Throws a JS Error with a descriptive message on failure.
#[wasm_bindgen]
pub fn convert(svg_input: &str) -> std::result::Result<JsValue, JsError> {
    convert_inner(svg_input).map_err(|e| JsError::new(&e.0))
}

fn convert_inner(svg_input: &str) -> Result<JsValue> {
    let page_size = &PageSize::LETTER_LANDSCAPE;
    let dpi = 300.0;

    let (group_start, group_end) = find_cut_layer_bounds(svg_input)?;
    let paths = extract_cut_layer(svg_input, group_start, group_end)?;

    let print_svg = format!(
        "{}{}{}",
        &svg_input[..group_start],
        registration_marks::generate_embeddable_marks_inset_svg(
            page_size,
            Some(dpi),
            INSET_X,
            INSET_Y,
        ),
        &svg_input[group_end..]
    );

    let fcm = svg_to_print_and_cut_fcm(&paths, page_size, dpi)?;
    let fcm_bytes = fcm.to_bytes().map_err(|e| ConvertError(format!("Failed to serialize fcm: {e}")))?;

    fcm_to_bytes(fcm)?;

    // Build a JS array: [Uint8Array, String]
    let js_array = js_sys::Array::new();
    js_array.push(&js_sys::Uint8Array::from(fcm_bytes.as_slice()));
    js_array.push(&JsValue::from_str(&print_svg));

    Ok(js_array.into())
}

// ---------------------------------------------------------------------------
// FCM serialisation
// ---------------------------------------------------------------------------

/// Write an FcmFile into a Vec<u8> without touching the filesystem.
///
/// This mirrors what fcmlib's `to_file` does, but into an in-memory cursor.
/// If fcmlib exposes a `write<W: Write>` method you can call that directly;
/// otherwise use the Cursor approach below.
fn fcm_to_bytes(fcm: FcmFile) -> Result<Vec<u8>> {
    fcm.to_bytes()
        .map_err(|e| ConvertError(format!("Failed to serialise FCM: {e}")))
}

// ---------------------------------------------------------------------------
// SVG parsing helpers (ported from the example, panics → Results)
// ---------------------------------------------------------------------------

/// Find the byte range of the `<g id="Cut-Layer" …>…</g>` element.
/// Returns (start_of_opening_tag, end_of_closing_tag).
fn find_cut_layer_bounds(svg: &str) -> Result<(usize, usize)> {
    let group_start = svg
        .find(r#"id="Cut-Layer""#)
        .ok_or(r#"Cut-Layer group not found (expected id="Cut-Layer" on a <g> element)"#)?;

    let tag_start = svg[..group_start]
        .rfind('<')
        .ok_or("Malformed SVG: could not find opening < for Cut-Layer group")?;

    let tag_end = svg[group_start..]
        .find('>')
        .ok_or("Malformed SVG: Cut-Layer <g> tag is not closed")?
        + group_start
        + 1;

    let mut depth = 1usize;
    let mut pos = tag_end;

    let group_end = loop {
        let next_open  = svg[pos..].find("<g" ).map(|i| i + pos);
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
            _ => return Err("Malformed SVG: unmatched <g> tag inside Cut-Layer".into()),
        }
    };

    Ok((tag_start, group_end))
}

/// Extract the `d="…"` attribute values from every <path> inside the cut layer.
fn extract_cut_layer(svg: &str, group_start: usize, group_end: usize) -> Result<Vec<String>> {
    let content = &svg[group_start..group_end];
    let mut paths = Vec::new();
    let mut search = content;

    while let Some(d_pos) = search.find(r#" d=""#) {
        let rest = &search[d_pos + 4..];
        if let Some(d_end) = rest.find('"') {
            paths.push(rest[..d_end].to_string());
        }
        search = &search[d_pos + 4..];
    }

    if paths.is_empty() {
        return Err("No path data found in Cut-Layer group".into());
    }

    Ok(paths)
}

// ---------------------------------------------------------------------------
// Core FCM conversion (unchanged logic from the example)
// ---------------------------------------------------------------------------

fn svg_to_print_and_cut_fcm(
    svg_paths: &[String],
    page: &PageSize,
    svg_dpi: f64,
) -> Result<FcmFile> {
    let config = SvgConfig {
        dpi: svg_dpi,
        scale: 1.0,
        offset_x_mm: 0.0,
        offset_y_mm: 0.0,
    };

    let parser = SvgPathParser::new(config);
    let mut shapes = Vec::new();
    for path in svg_paths {
        shapes.extend(
            parser
                .parse(path)
                .map_err(|e| ConvertError(format!("SVG path parse error: {e}")))?,
        );
    }

    if shapes.is_empty() {
        return Err("No shapes produced from SVG paths".into());
    }

    // Bounding box
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

    let width    = (max_x - min_x) as u32;
    let height   = (max_y - min_y) as u32;
    let center_x = (min_x + max_x) as f32 / 2.0;
    let center_y = (min_y + max_y) as f32 / 2.0;

    let paths: Vec<Path> = shapes
        .into_iter()
        .map(|shape| Path {
            tool: PathTool::TOOL_CUT,
            shape: Some(fcmlib::PathShape {
                start: Point {
                    x: shape.start.x - center_x as i32,
                    y: shape.start.y - center_y as i32,
                },
                outlines: shape
                    .outlines
                    .into_iter()
                    .map(|outline| match outline {
                        fcmlib::Outline::Line(segments) => {
                            fcmlib::Outline::Line(
                                segments
                                    .into_iter()
                                    .map(|seg| fcmlib::SegmentLine {
                                        end: Point {
                                            x: seg.end.x - center_x as i32,
                                            y: seg.end.y - center_y as i32,
                                        },
                                    })
                                    .collect(),
                            )
                        }
                        fcmlib::Outline::Bezier(segments) => {
                            fcmlib::Outline::Bezier(
                                segments
                                    .into_iter()
                                    .map(|seg| fcmlib::SegmentBezier {
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
                                    })
                                    .collect(),
                            )
                        }
                    })
                    .collect(),
            }),
            rhinestone_diameter: None,
            rhinestones: vec![],
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
        paths,
    };

    let page_width  = (page.width_mm  * 100.0) as u32;
    let page_height = (page.height_mm * 100.0) as u32;

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
