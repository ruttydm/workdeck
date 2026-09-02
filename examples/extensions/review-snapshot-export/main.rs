fn main() {
    if let Err(error) = workdeck_examples::review_snapshot_export_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("review-snapshot-export extension: {error}");
        std::process::exit(1);
    }
}
