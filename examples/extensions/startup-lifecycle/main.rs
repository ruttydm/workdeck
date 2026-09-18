fn main() {
    if let Err(error) = workdeck_examples::startup_lifecycle_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("startup lifecycle extension: {error}");
        std::process::exit(1);
    }
}
