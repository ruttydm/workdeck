{
  description = "Workdeck Rust/Ratatui development and package flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    systems.url = "github:nix-systems/triplet";
  };

  outputs = {
    self,
    nixpkgs,
    systems,
    ...
  }: let
    lib = nixpkgs.lib;
    supportedSystems = import systems;
    forAllSystems = lib.genAttrs supportedSystems;
    perSystem = forAllSystems (
      system: let
        pkgs = import nixpkgs {inherit system;};
        workdeck = pkgs.callPackage ./nix/package.nix {};
      in {
        packages = {
          inherit workdeck;
          default = workdeck;
        };
        apps = {
          workdeck = {
            type = "app";
            program = "${workdeck}/bin/workdeck";
            meta.description = "Run Workdeck";
          };
          default = self.apps.${system}.workdeck;
        };
        checks = {
          package = workdeck;
        };
        devShells = {
          default = pkgs.callPackage ./nix/devShell.nix {};
        };
      }
    );
    systemOutput = name: lib.mapAttrs (_: value: value.${name}) perSystem;
  in {
    packages = systemOutput "packages";
    apps = systemOutput "apps";
    checks = systemOutput "checks";
    devShells = systemOutput "devShells";

    homeManagerModules = {
      workdeck = import ./nix/home-manager.nix;
      default = {pkgs, ...}: {
        imports = [self.homeManagerModules.workdeck];
        programs.workdeck.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      };
    };
  };
}
