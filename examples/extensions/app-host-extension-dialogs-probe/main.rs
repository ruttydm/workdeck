fn main() {
    if let Err(error) = workdeck_examples::app_host_extension_dialogs_probe_extension::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::BufWriter::new(std::io::stdout()),
    ) {
        eprintln!("app-host-extension-dialogs-probe extension: {error}");
        std::process::exit(1);
    }
}
