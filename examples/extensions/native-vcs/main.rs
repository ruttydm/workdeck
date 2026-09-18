fn main() {
    if let Err(error) = workdeck_examples::native_vcs_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("native VCS extension: {error}");
        std::process::exit(1);
    }
}
