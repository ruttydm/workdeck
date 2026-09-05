fn main() {
    if let Err(error) = workdeck_examples::keybindings_probe_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("keybindings-probe extension: {error}");
        std::process::exit(1);
    }
}
