// Quick FCM analysis script
// Run with: cd fcmlib && cargo run --example analyze

use fcmlib::FcmFile;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <fcm_file>", args[0]);
        std::process::exit(1);
    }

    let file_path = &args[1];

    match FcmFile::from_file(file_path) {
        Ok(fcm) => {
            println!("=== FCM File Analysis: {} ===\n", file_path);

            println!("--- File Header ---");
            println!("  Variant: {:?}", fcm.file_header.variant);
            println!("  Version: {}", fcm.file_header.version);
            println!("  Content ID: {}", fcm.file_header.content_id);
            println!("  Short Name: '{}'", fcm.file_header.short_name);
            println!("  Long Name: '{}'", fcm.file_header.long_name);
            println!("  Author: '{}'", fcm.file_header.author_name);
            println!("  Generator: {:?}", fcm.file_header.generator);
            println!("  Print-to-Cut Flag: {:?}", fcm.file_header.print_to_cut);
            println!("  Thumbnail Size: {}x{}",
                fcm.file_header.thumbnail_block_size_width,
                fcm.file_header.thumbnail_block_size_height);

            println!("\n--- Cut Data ---");
            println!("  File Type: {:?}", fcm.cut_data.file_type);
            println!("  Mat ID: {}", fcm.cut_data.mat_id);
            println!("  Cut Width: {} ({}mm)", fcm.cut_data.cut_width, fcm.cut_data.cut_width as f64 / 100.0);
            println!("  Cut Height: {} ({}mm)", fcm.cut_data.cut_height, fcm.cut_data.cut_height as f64 / 100.0);
            println!("  Seam Allowance Width: {}", fcm.cut_data.seam_allowance_width);

            if let Some(alignment) = &fcm.cut_data.alignment {
                println!("\n  --- Alignment Data (Registration Marks) ---");
                println!("    Needed: {}", alignment.needed);
                println!("    Number of marks: {}", alignment.marks.len());
                for (i, mark) in alignment.marks.iter().enumerate() {
                    println!("    Mark {}: ({}, {}) = ({:.2}mm, {:.2}mm)",
                        i, mark.x, mark.y,
                        mark.x as f64 / 100.0, mark.y as f64 / 100.0);
                }
            } else {
                println!("\n  No alignment data (not a print-and-cut file)");
            }

            println!("\n--- Piece Table ---");
            println!("  Number of pieces: {}", fcm.piece_table.pieces.len());

            for (i, (id, piece)) in fcm.piece_table.pieces.iter().enumerate() {
                println!("\n  Piece {} (id={}):", i, id);
                println!("    Dimensions: {}x{} ({}mm x {}mm)",
                    piece.width, piece.height,
                    piece.width as f64 / 100.0, piece.height as f64 / 100.0);
                println!("    Label: '{}'", piece.label);
                println!("    Transform: {:?}", piece.transform);
                println!("    Paths: {}", piece.paths.len());

                for (j, path) in piece.paths.iter().enumerate() {
                    println!("      Path {}: Tool={:?}", j, path.tool);
                    if let Some(shape) = &path.shape {
                        println!("        Start: ({}, {})", shape.start.x, shape.start.y);
                        println!("        Outlines: {}", shape.outlines.len());
                        for (k, outline) in shape.outlines.iter().enumerate() {
                            println!("          Outline {}: {:?}", k, outline);
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error parsing FCM file: {:?}", e);
            std::process::exit(1);
        }
    }
}
