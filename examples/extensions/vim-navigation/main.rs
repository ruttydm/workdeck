fn main() {
    if let Err(error) = workdeck_examples::vim_navigation_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("vim-navigation extension: {error}");
        std::process::exit(1);
    }
}
