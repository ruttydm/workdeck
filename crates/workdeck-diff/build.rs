use std::path::PathBuf;
use syntect::parsing::{SyntaxDefinition, SyntaxSet};

fn main() {
    println!("cargo:rerun-if-changed=assets/syntaxes/Elixir.sublime-syntax");
    let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
    builder.add(
        SyntaxDefinition::load_from_str(
            include_str!("assets/syntaxes/Elixir.sublime-syntax"),
            true,
            Some("Elixir"),
        )
        .expect("bundled Elixir syntax is valid"),
    );
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
    syntect::dumps::dump_to_uncompressed_file(&builder.build(), output.join("syntaxes.bin"))
        .expect("serialize bundled syntax set");
}
