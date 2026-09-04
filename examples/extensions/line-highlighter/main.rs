fn main() -> std::io::Result<()> {
    workdeck_examples::line_highlighter_extension::serve(
        std::io::BufReader::new(std::io::stdin().lock()),
        std::io::stdout().lock(),
    )
}
