fn main() {
    if let Err(error) = workdeck_examples::keyboard_probe_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("keyboard-probe extension: {error}");
        std::process::exit(1);
    }
}
