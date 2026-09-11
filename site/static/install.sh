#!/bin/sh
# Workdeck native archive installer. Copyright (c) Workdeck contributors; MIT.
set -eu

REPO="ruttydm/workdeck"
DOWNLOAD_BASE="https://github.com/${REPO}/releases/download"

info() { printf '%s\n' "$1"; }
fail() { printf 'error: %s\n' "$1" >&2; exit 1; }

detect_os() {
  case "$(uname -s)" in
    Darwin) printf 'darwin\n' ;;
    Linux) printf 'linux\n' ;;
    *) fail "Unsupported operating system: $(uname -s). Use Cargo or a native package manager." ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64)
      if [ "$(uname -s)" = Darwin ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
        printf 'aarch64-apple-darwin\n'
      else
        [ "$(uname -s)" = Darwin ] && printf 'x86_64-apple-darwin\n' || printf 'x86_64-unknown-linux-gnu\n'
      fi
      ;;
    arm64|aarch64)
      [ "$(uname -s)" = Darwin ] && printf 'aarch64-apple-darwin\n' || printf 'aarch64-unknown-linux-gnu\n'
      ;;
    *) fail "Unsupported architecture: $(uname -m). Use Cargo or a native package manager." ;;
  esac
}

main() {
  version="${WORKDECK_VERSION:-}"
  no_modify_path="${WORKDECK_NO_MODIFY_PATH:-0}"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      -h|--help) info "Install Workdeck, the terminal-native review workspace."; info "Usage: install.sh [version] [--no-modify-path]"; exit 0 ;;
      --no-modify-path) no_modify_path=1 ;;
      -*) fail "Unknown option: $1" ;;
      *) version="$1" ;;
    esac
    shift
  done
  version="${version#v}"
  # Guard the platform; the operating system name itself is not needed later.
  detect_os >/dev/null
  target="$(detect_arch)"
  [ -n "$version" ] || fail "WORKDECK_VERSION or a release version is required."
  if command -v curl >/dev/null 2>&1; then
    download() { curl -fsSL "$1" -o "$2"; }
  elif command -v wget >/dev/null 2>&1; then
    download() { wget -q -O "$2" "$1"; }
  else
    fail "Neither curl nor wget is available."
  fi
  archive="workdeck-${target}.tar.gz"
  case "$target" in *windows*) archive="workdeck-${target}.zip" ;; esac
  home="${HOME:-}"
  [ -n "$home" ] || fail "HOME is not set."
  root="${WORKDECK_INSTALL_DIR:-${home}/.workdeck}"
  temp="$(mktemp -d)"
  cleanup() { rm -rf "$temp"; }
  trap cleanup EXIT INT TERM
  info "Downloading ${archive} (v${version})..."
  download "${DOWNLOAD_BASE}/v${version}/${archive}" "${temp}/${archive}" || fail "Could not download ${archive}."
  download "${DOWNLOAD_BASE}/v${version}/${archive}.sha256" "${temp}/${archive}.sha256" || fail "Could not download checksum for ${archive}."
  (cd "$temp" && if command -v sha256sum >/dev/null 2>&1; then sha256sum -c "${archive}.sha256"; else shasum -a 256 -c "${archive}.sha256"; fi) || fail "Checksum verification failed."
  mkdir -p "${temp}/extract"
  case "$archive" in
    *.tar.gz) tar -xzf "${temp}/${archive}" -C "${temp}/extract" ;;
    *) fail "This installer supports Unix archives only; use the native Windows release." ;;
  esac
  package="${temp}/extract/workdeck-${target}"
  [ -f "${package}/workdeck" ] || fail "The release archive contains no Workdeck executable."
  mkdir -p "${root}/bin" "${root}/share/workdeck"
  install -m 0755 "${package}/workdeck" "${root}/bin/workdeck"
  if [ -d "${package}/skills" ]; then cp -R "${package}/skills" "${root}/share/workdeck/"; fi
  for metadata in LICENSE THIRD_PARTY_NOTICES licenses.json sbom.cdx.json provenance.json; do
    [ -f "${package}/${metadata}" ] && install -m 0644 "${package}/${metadata}" "${root}/${metadata}"
  done
  info "Installed Workdeck ${version} to ${root}/bin/workdeck"
  if [ "$no_modify_path" = 1 ]; then
    info "PATH was not modified; add ${root}/bin to PATH to run workdeck."
  else
    info "Add ${root}/bin to PATH in your shell startup file to run workdeck."
  fi
}

main "$@"
