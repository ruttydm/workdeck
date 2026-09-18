# Installing with Nix

The Workdeck flake builds the Rust/Ratatui product directly from `Cargo.lock`. It does not use Bun,
Node, npm, a JavaScript engine, or a WASM runtime.

## Install from a flake

Add Workdeck as a flake input and follow your existing `nixpkgs` pin:

```nix
inputs.workdeck = {
  url = "github:ruttydm/workdeck";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Use the package in NixOS or Home Manager:

```nix
environment.systemPackages = [
  inputs.workdeck.packages.${pkgs.stdenv.hostPlatform.system}.workdeck
];
```

```nix
home.packages = [
  inputs.workdeck.packages.${pkgs.stdenv.hostPlatform.system}.workdeck
];
```

Run without installing:

```bash
nix run github:ruttydm/workdeck -- --help
```

## Home Manager

The included module owns the package, Workdeck TOML configuration, optional Git/Jujutsu pager
integration, and optional Claude skill link:

```nix
{
  imports = [inputs.workdeck.homeManagerModules.default];

  programs.workdeck = {
    enable = true;
    enableGitIntegration = true;
    enableJujutsuIntegration = true;
    enableClaudeIntegration = true;
    settings = {
      ui.theme = "graphite";
      review = {
        mode = "split";
        line_numbers = true;
        tab_width = 4;
      };
    };
  };
}
```

Git integration requires Home Manager's Git module (`programs.git.enable = true`).

## Build and verify

```bash
nix build .#workdeck
cargo xtask nix check
```

The xtask evaluates the complete flake, builds its check package, and runs the installed-style
`workdeck --help` smoke without updating `flake.lock`. The CI Nix job runs this gate on every change.

## Supported systems

The default `nix-systems/triplet` input exposes `x86_64-linux`, `aarch64-linux`, and
`aarch64-darwin`. Nixpkgs 26.11 no longer evaluates `x86_64-darwin`; Intel macOS remains supported
by Workdeck's Cargo, Homebrew, and direct GitHub artifacts. Consumers pinning an older Nixpkgs may
override `inputs.workdeck.inputs.systems` with a wider system list.

Update dependencies with Cargo and commit both Rust manifests and `Cargo.lock`. There is no separate
Nix dependency lock derivation.
