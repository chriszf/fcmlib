// Scan all sample FCM files to find print-and-cut examples
// Run with: cargo run --example find_print_and_cut

use fcmlib::FcmFile;
use std::fs;
use std::path::Path;

fn main() {
    let samples_dir = Path::new("tests/samples/brother");

    if !samples_dir.exists() {
        eprintln!("Samples directory not found: {:?}", samples_dir);
        return;
    }

    let mut print_and_cut_files = Vec::new();
    let mut total_files = 0;

    for entry in fs::read_dir(samples_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().map_or(false, |e| e == "fcm") {
            total_files += 1;

            if let Ok(fcm) = FcmFile::from_file(&path) {
                if let Some(alignment) = &fcm.cut_data.alignment {
                    print_and_cut_files.push((
                        path.file_name().unwrap().to_string_lossy().to_string(),
                        fcm.cut_data.file_type,
                        alignment.needed,
                        alignment.marks.len(),
                        fcm.file_header.variant,
                    ));
                }
            }
        }
    }

    println!("Scanned {} FCM files", total_files);
    println!("\nPrint-and-Cut files found: {}\n", print_and_cut_files.len());

    for (name, file_type, needed, mark_count, variant) in &print_and_cut_files {
        println!("  {} - {:?}, variant={:?}, needed={}, marks={}",
            name, file_type, variant, needed, mark_count);
    }
}
