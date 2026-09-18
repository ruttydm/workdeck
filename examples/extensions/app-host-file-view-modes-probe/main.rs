fn main() {
    if let Err(error) = workdeck_examples::app_host_file_view_modes_probe_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("app-host-file-view-modes-probe extension: {error}");
        std::process::exit(1);
    }
}
