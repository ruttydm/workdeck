fn main() {
    if let Err(error) = workdeck_examples::cli_tools_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("cli-tools extension: {error}");
        std::process::exit(1);
    }
}
