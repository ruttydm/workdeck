{
  lib,
  stdenv,
  rustPlatform,
  pkg-config,
  git,
  openssl,
  zlib,
  libiconv,
}:
rustPlatform.buildRustPackage {
  pname = "workdeck";
  version = (lib.importTOML ../crates/workdeck-cli/Cargo.toml).package.version;

  src = lib.cleanSourceWith {
    src = ../.;
    filter = path: type:
      let base = baseNameOf path;
      in !(base == "target" || base == "result" || base == ".agents");
  };

  cargoLock.lockFile = ../Cargo.lock;
  cargoBuildFlags = ["--package" "workdeck-cli" "--bin" "workdeck"];
  doCheck = false;

  nativeBuildInputs = [pkg-config];
  nativeCheckInputs = [git];
  buildInputs = [openssl zlib] ++ lib.optionals stdenv.hostPlatform.isDarwin [libiconv];

  postInstall = ''
    mkdir -p "$out/share/workdeck" "$out/share/doc/workdeck"
    cp -R skills "$out/share/workdeck/skills"
    cp LICENSE THIRD_PARTY_NOTICES "$out/share/doc/workdeck/"
  '';

  meta = {
    description = "Terminal-native review and repository workbench";
    homepage = "https://github.com/ruttydm/workdeck";
    license = lib.licenses.mit;
    mainProgram = "workdeck";
    platforms = ["x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"];
  };
}
