fn main() {
    if let Err(error) = workdeck_examples::current_line_lens_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("current-line-lens extension: {error}");
        std::process::exit(1);
    }
}
