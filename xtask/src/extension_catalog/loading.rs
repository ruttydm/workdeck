//! MIT port of the build-time activity loader in Hunk's extension directory.
use super::{activity, index_activity};
use anyhow::{Result, bail};
use serde_json::Value;
use std::time::Duration;

pub(super) fn load(payload: &Value) -> Result<Vec<Value>> {
    let entries = payload
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("load expects an array of listings"))?;
    for entry in entries {
        if entry.get("repo").and_then(Value::as_str).is_none() {
            bail!("each listing requires a repository string");
        }
    }
    let token = std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|value| !value.is_empty());
    let fetch = |path: &str, timeout: Duration| -> Result<Value> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build()
            .into();
        let mut request = agent
            .get(format!("https://api.github.com{path}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "workdeck-extension-directory");
        if let Some(token) = &token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let mut response = request.call()?;
        Ok(serde_json::from_str(
            &response.body_mut().read_to_string()?,
        )?)
    };
    let (entries, warnings) = resolve(entries, "workdeck-extension", &fetch);
    for warning in warnings {
        eprintln!("{warning}");
    }
    Ok(entries)
}

fn resolve(
    entries: &[Value],
    topic: &str,
    fetch: &(impl Fn(&str, Duration) -> Result<Value> + Sync),
) -> (Vec<Value>, Vec<String>) {
    let query = format!("topic%3A{topic}%20is%3Apublic");
    let mut warnings = Vec::new();
    let tagged = match fetch(
        &format!("/search/repositories?q={query}&per_page=100"),
        Duration::from_secs(8),
    ) {
        Ok(payload) => index_activity(&payload),
        Err(_) => {
            // Never log transport error details that might contain credentials.
            warnings.push(
                "Extension directory: GitHub topic search failed; rendering without stars.".into(),
            );
            Default::default()
        }
    };
    let missing: Vec<_> = entries
        .iter()
        .filter(|entry| !tagged.contains_key(&entry["repo"].as_str().unwrap().to_lowercase()))
        .collect();
    if !tagged.is_empty() && !missing.is_empty() {
        warnings.push(format!(
            "Extension directory: {} {} listed but not tagged `{topic}`.",
            missing
                .iter()
                .map(|entry| entry["repo"].as_str().unwrap())
                .collect::<Vec<_>>()
                .join(", "),
            if missing.len() == 1 { "is" } else { "are" }
        ));
    }
    let direct: std::collections::BTreeMap<_, _> = std::thread::scope(|scope| {
        let handles: Vec<_> = missing
            .iter()
            .map(|entry| {
                let repo = entry["repo"].as_str().unwrap();
                scope.spawn(move || {
                    let data = fetch(&format!("/repos/{repo}"), Duration::from_secs(5))
                        .map(|value| activity(&value))
                        .unwrap_or_else(|_| serde_json::json!({}));
                    (repo, data)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("directory fetch worker panicked"))
            .collect()
    });
    let result = entries
        .iter()
        .map(|entry| {
            let repo = entry["repo"].as_str().unwrap();
            let mut entry = entry.clone();
            if let Some(metadata) = tagged
                .get(&repo.to_lowercase())
                .or_else(|| direct.get(repo))
            {
                entry
                    .as_object_mut()
                    .unwrap()
                    .extend(metadata.as_object().unwrap().clone());
            }
            entry
        })
        .collect();
    (result, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Barrier, Mutex};

    #[test]
    fn topic_match_and_parallel_fallback_preserve_order_and_missing_fields() {
        let calls = Mutex::new(Vec::new());
        let barrier = Barrier::new(2);
        let fetch = |path: &str, timeout| {
            calls.lock().unwrap().push((path.to_string(), timeout));
            if path.starts_with("/search/") {
                return Ok(json!({"items":[{"full_name":"OWNER/TAGGED","stargazers_count":0}]}));
            }
            barrier.wait(); // Both fallback requests must be in flight together.
            if path == "/repos/owner/missing" {
                Ok(json!({"pushed_at":"2020-01-01"}))
            } else {
                bail!("offline")
            }
        };
        let entries = vec![
            json!({"repo":"owner/missing","name":"first"}),
            json!({"repo":"Owner/Tagged"}),
            json!({"repo":"owner/offline"}),
        ];
        let (result, warnings) = resolve(&entries, "workdeck-extension", &fetch);
        assert_eq!(
            result,
            vec![
                json!({"repo":"owner/missing","name":"first","pushedAt":"2020-01-01"}),
                json!({"repo":"Owner/Tagged","stars":0}),
                entries[2].clone()
            ]
        );
        assert_eq!(
            warnings,
            [
                "Extension directory: owner/missing, owner/offline are listed but not tagged `workdeck-extension`."
            ]
        );
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(
            calls[0],
            (
                "/search/repositories?q=topic%3Aworkdeck-extension%20is%3Apublic&per_page=100"
                    .into(),
                Duration::from_secs(8)
            )
        );
        assert!(
            calls[1..]
                .iter()
                .all(|(_, timeout)| *timeout == Duration::from_secs(5))
        );
    }

    #[test]
    fn failed_or_empty_topic_still_fetches_direct_without_untagged_warning() {
        for failed in [false, true] {
            let (result, warnings) = resolve(
                &[json!({"repo":"owner/repo"})],
                "workdeck-extension",
                &|path, _| {
                    if path.starts_with("/search/") {
                        if failed {
                            bail!("sensitive transport detail")
                        }
                        Ok(json!({"items":"malformed"}))
                    } else {
                        Ok(json!({"stargazers_count":7}))
                    }
                },
            );
            assert_eq!(result, [json!({"repo":"owner/repo","stars":7})]);
            assert_eq!(warnings.len(), usize::from(failed));
            assert!(
                warnings.iter().all(
                    |warning| !warning.contains("sensitive") && !warning.contains("not tagged")
                )
            );
        }
    }
}
