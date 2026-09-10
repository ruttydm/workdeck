//! Native prebuilt metadata derived from Hunk's MIT build-prebuilt-artifact.ts.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrebuiltMetadata {
    pub package_name: String,
    pub os: String,
    pub cpu: String,
    pub binary_name: String,
}

impl PrebuiltMetadata {
    pub fn for_target(target: &str) -> Result<Self> {
        let (os, cpu, binary) = match target {
            "aarch64-apple-darwin" => ("darwin", "arm64", "workdeck"),
            "x86_64-apple-darwin" => ("darwin", "x64", "workdeck"),
            "aarch64-unknown-linux-gnu" => ("linux", "arm64", "workdeck"),
            "x86_64-unknown-linux-gnu" => ("linux", "x64", "workdeck"),
            "x86_64-pc-windows-msvc" => ("windows", "x64", "workdeck.exe"),
            _ => bail!("unsupported release package target {target:?}"),
        };
        Ok(Self {
            package_name: format!("workdeck-{target}"),
            os: os.into(),
            cpu: cpu.into(),
            binary_name: binary.into(),
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec_pretty(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8], target: &str) -> Result<Self> {
        ensure!(
            bytes.len() <= 64 * 1024,
            "prebuilt metadata exceeds size limit"
        );
        let metadata: Self = serde_json::from_slice(bytes)?;
        ensure!(
            metadata == Self::for_target(target)?,
            "prebuilt metadata does not match release target"
        );
        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_rejects_mismatched_duplicate_unknown_and_oversized_fields() {
        let target = "aarch64-apple-darwin";
        let expected = PrebuiltMetadata::for_target(target).unwrap();
        assert_eq!(
            PrebuiltMetadata::decode(&expected.encode().unwrap(), target).unwrap(),
            expected
        );
        let mut wrong = expected.clone();
        wrong.binary_name = "../workdeck".into();
        assert!(PrebuiltMetadata::decode(&wrong.encode().unwrap(), target).is_err());
        assert!(
            PrebuiltMetadata::decode(&expected.encode().unwrap(), "x86_64-apple-darwin").is_err()
        );
        let json = String::from_utf8(expected.encode().unwrap()).unwrap();
        for field in ["\"cpu\":\"arm64\",", "\"unknown\":true,"] {
            assert!(
                PrebuiltMetadata::decode(
                    json.replacen('{', &format!("{{{field}"), 1).as_bytes(),
                    target
                )
                .is_err()
            );
        }
        assert!(PrebuiltMetadata::decode(&vec![b' '; 65537], target).is_err());
    }
}
