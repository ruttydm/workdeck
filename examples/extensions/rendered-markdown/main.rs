fn main() {
    if let Err(error) = workdeck_examples::rendered_markdown_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("rendered-markdown extension: {error}");
        std::process::exit(1);
    }
}
