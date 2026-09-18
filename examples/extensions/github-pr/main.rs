fn main() -> std::io::Result<()> {
    workdeck_examples::github_pr_extension::serve(
        std::io::BufReader::new(std::io::stdin().lock()),
        std::io::stdout(),
    )
}
