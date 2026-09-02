fn main() {
    if let Err(error) = workdeck_examples::review_note_navigator_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("review-note-navigator extension: {error}");
        std::process::exit(1);
    }
}
