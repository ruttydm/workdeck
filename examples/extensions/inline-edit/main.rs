fn main() {
    if let Err(error) = workdeck_examples::inline_edit_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("inline-edit extension: {error}");
        std::process::exit(1);
    }
}
