fn main() {
    if let Err(error) = workdeck_examples::jsx_file_view_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("jsx-file-view extension: {error}");
        std::process::exit(1);
    }
}
