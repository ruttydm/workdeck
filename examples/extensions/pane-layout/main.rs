fn main() {
    if let Err(error) = workdeck_examples::pane_layout_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("pane-layout extension: {error}");
        std::process::exit(1);
    }
}
