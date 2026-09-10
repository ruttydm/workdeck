//! Resolve release tags independently of untrusted package metadata.
use anyhow::{Result, ensure};
use serde_json::Value;
use std::collections::BTreeSet;

pub struct ResolvedRelease {
    pub version: String,
    pub commit: String,
    pub tag_ref: String,
}

pub fn resolve_release_identity(version: &str) -> Result<ResolvedRelease> {
    let started = std::time::Instant::now();
    resolve_with(version, |path| {
        let remaining = std::time::Duration::from_secs(30)
            .checked_sub(started.elapsed())
            .ok_or_else(|| anyhow::anyhow!("release identity lookup deadline exceeded"))?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(remaining))
            .max_redirects(0)
            .build()
            .into();
        let mut response = agent
            .get(&format!(
                "https://api.github.com/repos/ruttydm/workdeck{path}"
            ))
            .header("User-Agent", "workdeck-update")
            .header("Accept", "application/vnd.github+json")
            .call()?;
        ensure!(
            response.status().is_success(),
            "release identity HTTP request failed"
        );
        let bytes = response
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_to_vec()?;
        Ok(serde_json::from_slice(&bytes)?)
    })
}

fn resolve_with(
    version: &str,
    mut fetch: impl FnMut(&str) -> Result<Value>,
) -> Result<ResolvedRelease> {
    let version = crate::update::parse_update_version(version)?;
    let tag_ref = format!("refs/tags/v{version}");
    let reference = fetch(&format!("/git/ref/tags/v{version}"))?;
    ensure!(
        reference["ref"].as_str() == Some(&tag_ref),
        "release lookup returned a different tag"
    );
    let mut object = reference["object"].clone();
    let mut visited = BTreeSet::new();
    for _ in 0..=8 {
        let sha = object["sha"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("release object has no digest"))?;
        ensure!(
            sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid GitHub release object digest"
        );
        ensure!(
            visited.insert(sha.to_ascii_lowercase()),
            "cyclic release tag chain"
        );
        match object["type"].as_str() {
            Some("commit") => {
                return Ok(ResolvedRelease {
                    version,
                    commit: sha.to_ascii_lowercase(),
                    tag_ref,
                });
            }
            Some("tag") => {
                ensure!(
                    visited.len() <= 8,
                    "release tag chain exceeds eight annotations"
                );
                let tag = fetch(&format!("/git/tags/{sha}"))?;
                ensure!(
                    tag["sha"]
                        .as_str()
                        .is_some_and(|returned| returned.eq_ignore_ascii_case(sha)),
                    "tag lookup returned a different object"
                );
                object = tag["object"].clone();
            }
            _ => anyhow::bail!("release tag does not resolve to a commit"),
        }
    }
    anyhow::bail!("release tag chain exceeds limit")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_lightweight_and_annotated_tags_without_trusting_package_metadata() {
        for annotated in [false, true] {
            let commit = "a".repeat(40);
            let tag = "b".repeat(40);
            let mut paths = Vec::new();
            let resolved = resolve_with("v1.2.3", |path| {
                paths.push(path.to_owned());
                if path.starts_with("/git/ref/") {
                    Ok(json!({"ref":"refs/tags/v1.2.3", "object":{"type":if annotated { "tag" } else { "commit" },"sha":if annotated { &tag } else { &commit }}}))
                } else {
                    assert_eq!(path, format!("/git/tags/{tag}"));
                    Ok(json!({"sha":tag,"object":{"type":"commit","sha":commit}}))
                }
            }).unwrap();
            assert_eq!(resolved.version, "1.2.3");
            assert_eq!(resolved.commit, commit);
            assert_eq!(resolved.tag_ref, "refs/tags/v1.2.3");
            assert_eq!(paths.len(), if annotated { 2 } else { 1 });
        }
    }

    #[test]
    fn rejects_wrong_refs_noncommits_cycles_and_malformed_digests() {
        for (reference, kind, sha) in [
            ("refs/tags/v9", "commit", "a".repeat(40)),
            ("refs/tags/v1.2.3", "tree", "a".repeat(40)),
            ("refs/tags/v1.2.3", "commit", "short".into()),
            ("refs/tags/v1.2.3", "tag", "a".repeat(40)),
        ] {
            assert!(
                resolve_with("1.2.3", |_| Ok(
                    json!({"ref":reference,"sha":sha,"object":{"type":kind,"sha":sha}})
                ))
                .is_err()
            );
        }
    }
}
