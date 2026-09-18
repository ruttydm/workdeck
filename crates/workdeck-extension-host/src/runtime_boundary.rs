//! Native extension entrypoint resolution.
//!
//! Hunk had to rewrite imports in dynamically loaded TypeScript so extension code shared the
//! host's React and OpenTUI instances. Workdeck's extension boundary is process based instead:
//! one manifest selects one compiled executable, and all host-owned values cross the versioned
//! JSON-RPC protocol as owned data. Keeping resolution in this module makes that boundary
//! explicit and prevents adjacent source files or package managers from becoming runtime inputs.

use crate::HostError;
use std::fs;
use std::path::{Path, PathBuf};
use workdeck_extension_api::ExtensionManifest;

/// Fully resolved process boundary for one native extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeExtensionEntrypoint {
    pub manifest: ExtensionManifest,
    /// Canonical identity used for diagnostics, reload comparison, and duplicate containment.
    pub manifest_path: PathBuf,
    /// Canonical working directory inherited by the child process.
    pub directory: PathBuf,
    /// Exact executable selected by the manifest, with the platform suffix fallback applied.
    pub executable: PathBuf,
}

/// Resolve a native manifest without inspecting JavaScript, package metadata, or source imports.
pub fn resolve_native_extension_entrypoint(
    manifest_path: &Path,
) -> Result<NativeExtensionEntrypoint, HostError> {
    let manifest = ExtensionManifest::load(manifest_path)?;
    let resolved_manifest_path =
        fs::canonicalize(manifest_path).unwrap_or_else(|_| manifest_path.to_owned());
    let authored_directory = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let directory =
        fs::canonicalize(authored_directory).unwrap_or_else(|_| authored_directory.to_owned());
    let executable = resolve_existing_executable(
        &directory,
        &manifest.executable,
        std::env::consts::EXE_SUFFIX,
    )?;

    Ok(NativeExtensionEntrypoint {
        manifest,
        manifest_path: resolved_manifest_path,
        directory,
        executable,
    })
}

fn resolve_existing_executable(
    directory: &Path,
    relative: &Path,
    executable_suffix: &str,
) -> Result<PathBuf, HostError> {
    let executable = directory.join(relative);
    if executable.is_file() {
        return Ok(executable);
    }
    if !executable_suffix.is_empty() {
        let mut name = executable.as_os_str().to_owned();
        name.push(executable_suffix);
        let suffixed = PathBuf::from(name);
        if suffixed.is_file() {
            return Ok(suffixed);
        }
    }
    Err(HostError::MissingExecutable(executable))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_native_extension(directory: &Path) -> (PathBuf, PathBuf) {
        fs::create_dir_all(directory.join("bin")).unwrap();
        let executable = directory.join("bin/review-native");
        fs::write(&executable, b"compiled fixture").unwrap();
        let manifest = directory.join("workdeck-extension.toml");
        fs::write(
            &manifest,
            "id = 'review.native'\nname = 'Review native'\nversion = '1.0.0'\napi_version = 1\nexecutable = 'bin/review-native'\ncapabilities = ['file-views']\n",
        )
        .unwrap();
        (manifest, executable)
    }

    #[test]
    fn manifest_owned_native_entrypoint_ignores_adjacent_javascript_runtime_files() {
        let root = TempDir::new().unwrap();
        let extension = root.path().join("extension");
        let (manifest_path, executable) = write_native_extension(&extension);
        fs::create_dir_all(extension.join("node_modules/react")).unwrap();
        fs::write(
            extension.join("node_modules/react/index.js"),
            "module.exports = { useState() { throw Error('wrong runtime') } };\n",
        )
        .unwrap();
        fs::write(extension.join("index.tsx"), "import React from 'react';\n").unwrap();
        fs::write(extension.join("helper.ts"), "export const helper = 1;\n").unwrap();
        fs::write(
            extension.join("package.json"),
            r#"{"main":"index.tsx","dependencies":{"react":"0.0.1"}}"#,
        )
        .unwrap();

        let resolved = resolve_native_extension_entrypoint(&manifest_path).unwrap();
        assert_eq!(resolved.manifest.id, "review.native");
        assert_eq!(resolved.directory, fs::canonicalize(&extension).unwrap());
        assert_eq!(resolved.executable, fs::canonicalize(executable).unwrap());
        assert_eq!(
            resolved.manifest_path,
            fs::canonicalize(&manifest_path).unwrap()
        );
    }

    #[test]
    fn platform_executable_suffix_is_a_fallback_not_a_second_entrypoint() {
        let root = TempDir::new().unwrap();
        let base = root.path().join("extension");
        let suffixed = root.path().join("extension.exe");
        fs::write(&suffixed, b"windows fixture").unwrap();
        assert_eq!(
            resolve_existing_executable(root.path(), Path::new("extension"), ".exe").unwrap(),
            suffixed
        );

        fs::write(&base, b"exact fixture").unwrap();
        assert_eq!(
            resolve_existing_executable(root.path(), Path::new("extension"), ".exe").unwrap(),
            base
        );
    }

    #[cfg(unix)]
    #[test]
    fn aliased_extension_root_resolves_to_one_canonical_native_boundary() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().unwrap();
        let actual = root.path().join("actual");
        let (actual_manifest, executable) = write_native_extension(&actual);
        let alias = root.path().join("alias");
        symlink(&actual, &alias).unwrap();

        let resolved =
            resolve_native_extension_entrypoint(&alias.join("workdeck-extension.toml")).unwrap();
        assert_eq!(resolved.directory, fs::canonicalize(&actual).unwrap());
        assert_eq!(
            resolved.manifest_path,
            fs::canonicalize(actual_manifest).unwrap()
        );
        assert_eq!(resolved.executable, fs::canonicalize(executable).unwrap());
    }

    #[test]
    fn missing_native_binary_is_attributed_to_the_manifest_selected_path() {
        let root = TempDir::new().unwrap();
        let extension = root.path().join("extension");
        let (manifest_path, executable) = write_native_extension(&extension);
        fs::remove_file(&executable).unwrap();
        let expected = fs::canonicalize(&extension)
            .unwrap()
            .join("bin/review-native");
        assert!(matches!(
            resolve_native_extension_entrypoint(&manifest_path),
            Err(HostError::MissingExecutable(path)) if path == expected
        ));
    }

    #[test]
    fn frozen_hunk_runtime_module_oracle_maps_every_source_test() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-host-runtime-modules.json"
        ))
        .unwrap();
        let baselines = oracle["baselines"].as_array().unwrap();
        assert_eq!(baselines.len(), 2);
        assert_eq!(baselines[0]["tests"], 8);
        assert_eq!(baselines[0]["passed"], 8);
        assert_eq!(baselines[0]["failed"], 0);
        assert_eq!(baselines[1]["tests"], 7);
        assert_eq!(baselines[1]["passed"], 6);
        assert_eq!(baselines[1]["failed"], 1);
        let mappings = oracle["test_mapping"].as_array().unwrap();
        assert_eq!(mappings.len(), 8);
        assert!(mappings.iter().all(|mapping| {
            mapping["source_test"]
                .as_str()
                .is_some_and(|name| !name.is_empty())
                && mapping["rust_tests"]
                    .as_array()
                    .is_some_and(|tests| !tests.is_empty())
        }));
    }
}
