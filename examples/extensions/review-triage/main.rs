fn main() -> std::io::Result<()> {
    workdeck_examples::review_triage_extension::serve(
        std::io::BufReader::new(std::io::stdin().lock()),
        std::io::stdout().lock(),
    )
}
