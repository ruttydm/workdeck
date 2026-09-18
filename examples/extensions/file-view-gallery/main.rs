fn main() {
    workdeck_examples::file_view_gallery_extension::serve(
        std::io::BufReader::new(std::io::stdin().lock()),
        std::io::stdout().lock(),
    )
    .expect("file-view gallery extension protocol failed");
}
