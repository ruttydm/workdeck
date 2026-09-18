{pkgs}:
pkgs.mkShell {
  packages = with pkgs; [
    cargo
    cargo-deny
    clippy
    git
    openssl
    pkg-config
    rustc
    rustfmt
    zlib
    zola
  ];
}
