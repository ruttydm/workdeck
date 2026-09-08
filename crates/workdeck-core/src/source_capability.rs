//! Provider-neutral identity metadata for a lazy source capability.
//!
//! Derived from Hunk's MIT-licensed `src/core/review/document.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. This descriptor is not an
//! executable fetcher and cannot authorize filesystem or provider access.

use serde::{Deserialize, Serialize};

/// Presence of this descriptor means a source capability exists, even before
/// either side has been loaded. An absent cache key is distinct from no capability.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCapabilityIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
}

impl SourceCapabilityIdentity {
    /// A present key, including an empty key, attests the provider's snapshot.
    #[must_use]
    pub const fn attested(&self) -> bool {
        self.cache_key.is_some()
    }

    #[must_use]
    pub fn source_identity(&self, path: &str, content_identity: &str) -> String {
        crate::review_source_identity(path, content_identity, self.cache_key.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_identity_and_attestation_match_both_pinned_projectors() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/source-capability-identity.json"
        ))
        .unwrap();
        assert_eq!(oracle["runs"].as_array().unwrap().len(), 2);
        for run in oracle["runs"].as_array().unwrap() {
            assert_eq!(run["exitCode"], 0);
            assert_eq!(run["cases"].as_array().unwrap().len(), 8);
            for case in run["cases"].as_array().unwrap() {
                let input = &case["input"];
                let capability = input
                    .get("capability")
                    .map(|value| SourceCapabilityIdentity {
                        cache_key: value
                            .get("cacheKey")
                            .and_then(|key| key.as_str())
                            .map(str::to_owned),
                    });
                let actual = capability.as_ref().map(|capability| {
                    capability.source_identity(
                        input
                            .get("path")
                            .and_then(|path| path.as_str())
                            .unwrap_or("source.ts"),
                        case["contentIdentity"].as_str().unwrap(),
                    )
                });
                assert_eq!(
                    actual.as_deref(),
                    case.get("sourceIdentity").and_then(|id| id.as_str()),
                    "{} at {}",
                    input["name"],
                    run["pin"]
                );
                assert_eq!(
                    capability.as_ref().map(SourceCapabilityIdentity::attested),
                    case.get("sourceAttested").and_then(|value| value.as_bool())
                );
                assert_eq!(case["reads"], 0);
            }
        }
    }

    #[test]
    fn serialization_preserves_absent_empty_and_named_cache_keys() {
        for cache_key in [None, Some(String::new()), Some("snapshot".into())] {
            let identity = SourceCapabilityIdentity { cache_key };
            let value = serde_json::to_value(&identity).unwrap();
            assert_eq!(value.get("cache_key").is_some(), identity.attested());
            assert_eq!(
                serde_json::from_value::<SourceCapabilityIdentity>(value).unwrap(),
                identity
            );
        }
        assert_eq!(
            serde_json::from_str::<SourceCapabilityIdentity>("{}").unwrap(),
            SourceCapabilityIdentity::default()
        );
    }
}
