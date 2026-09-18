//! Canonical repository trust decisions for native extensions.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use workdeck_core::resolve_canonical_path;

pub const EXTENSION_TRUST_STATE_KEY: &str = "extensionTrust";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustStore {
    #[serde(default)]
    pub repositories: BTreeMap<String, TrustDecision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustDecision {
    Trusted,
    Denied,
    /// Migrated provenance is deliberately not executable authority.
    Legacy,
}

impl TrustStore {
    /// Project recognized decisions from the shared Workdeck app-state record.
    #[must_use]
    pub fn from_app_state_record(record: &Map<String, Value>) -> Self {
        let mut store = Self::default();
        store.merge_app_state_record(record);
        store
    }

    /// Merge recognized decisions without allowing malformed state to erase earlier sources.
    pub fn merge_app_state_record(&mut self, record: &Map<String, Value>) {
        let Some(Value::Object(repositories)) = record.get(EXTENSION_TRUST_STATE_KEY) else {
            return;
        };
        for (repo_root, decision) in repositories {
            let decision = match decision.as_str() {
                Some("trusted") => TrustDecision::Trusted,
                Some("denied") => TrustDecision::Denied,
                Some("legacy") => TrustDecision::Legacy,
                _ => continue,
            };
            self.repositories.insert(repo_root.clone(), decision);
        }
    }

    /// Produce the top-level state patch used by a preserving app-state update.
    #[must_use]
    pub fn app_state_patch(&self) -> Map<String, Value> {
        let repositories = self
            .repositories
            .iter()
            .map(|(repo_root, decision)| {
                let decision = match decision {
                    TrustDecision::Trusted => "trusted",
                    TrustDecision::Denied => "denied",
                    TrustDecision::Legacy => "legacy",
                };
                (repo_root.clone(), Value::String(decision.into()))
            })
            .collect();
        Map::from_iter([(
            EXTENSION_TRUST_STATE_KEY.into(),
            Value::Object(repositories),
        )])
    }

    /// Import the pre-app-state Workdeck trust file without making it the new authority.
    #[must_use]
    pub fn load_legacy_toml(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|source| toml::from_str(&source).ok())
            .unwrap_or_default()
    }

    /// Retained for existing migration tooling; new decisions use shared app state.
    pub fn save(&self, path: &Path) -> Result<(), io::Error> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let encoded = toml::to_string_pretty(self).expect("trust store is TOML serializable");
        let temporary = path.with_extension("toml.tmp");
        fs::write(&temporary, encoded)?;
        fs::rename(temporary, path)
    }

    /// Resolve canonical decisions first and pre-canonicalization keys second.
    #[must_use]
    pub fn decision(&self, repo: &Path) -> Option<TrustDecision> {
        self.repositories
            .get(&canonical_trust_key(repo))
            .or_else(|| self.repositories.get(&legacy_resolved_key(repo)))
            .copied()
    }

    pub fn grant(&mut self, repo: &Path, decision: TrustDecision) {
        self.repositories
            .insert(canonical_trust_key(repo), decision);
    }
}

fn canonical_trust_key(path: &Path) -> String {
    resolve_canonical_path(path)
        .unwrap_or_else(|_| lexical_absolute_path(path))
        .to_string_lossy()
        .into_owned()
}

fn legacy_resolved_key(path: &Path) -> String {
    lexical_absolute_path(path).to_string_lossy().into_owned()
}

fn lexical_absolute_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn record(value: Value) -> Map<String, Value> {
        value.as_object().expect("test record").clone()
    }

    #[test]
    fn app_state_round_trips_only_recognized_decisions() {
        let source = record(json!({
            "extensionTrust": {
                "/repo/trusted": "trusted",
                "/repo/denied": "denied",
                "/repo/legacy": "legacy",
                "/repo/invalid": "maybe"
            }
        }));
        let trust = TrustStore::from_app_state_record(&source);
        assert_eq!(trust.repositories.len(), 3);
        assert_eq!(
            trust.app_state_patch()["extensionTrust"]["/repo/trusted"],
            "trusted"
        );
        assert!(
            trust.app_state_patch()["extensionTrust"]
                .get("/repo/invalid")
                .is_none()
        );
    }

    #[test]
    fn absent_and_wrongly_typed_state_are_empty() {
        for source in [
            json!({}),
            json!({"extensionTrust": null}),
            json!({"extensionTrust": []}),
        ] {
            assert!(
                TrustStore::from_app_state_record(&record(source))
                    .repositories
                    .is_empty()
            );
        }
    }

    #[test]
    fn canonical_keys_join_aliases_and_legacy_resolved_keys_remain_readable() {
        let root = TempDir::new().unwrap();
        let canonical = root.path().join("repo");
        fs::create_dir(&canonical).unwrap();
        let alias = root.path().join("alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&canonical, &alias).unwrap();
        #[cfg(windows)]
        if std::os::windows::fs::symlink_dir(&canonical, &alias).is_err() {
            return;
        }

        let mut trust = TrustStore::default();
        trust.grant(&alias, TrustDecision::Trusted);
        assert_eq!(trust.decision(&canonical), Some(TrustDecision::Trusted));
        assert_eq!(trust.decision(&alias), Some(TrustDecision::Trusted));
        assert_eq!(
            trust.repositories.keys().collect::<Vec<_>>(),
            [&canonical_trust_key(&canonical)]
        );

        let legacy = TrustStore {
            repositories: BTreeMap::from([(legacy_resolved_key(&alias), TrustDecision::Denied)]),
        };
        assert_eq!(legacy.decision(&alias), Some(TrustDecision::Denied));
    }

    #[test]
    fn app_state_patch_is_scoped_for_a_top_level_preserving_merge() {
        let root = TempDir::new().unwrap();
        let mut trust = TrustStore::default();
        trust.grant(root.path(), TrustDecision::Trusted);
        let mut state = record(json!({"version": 1, "lastSeenCliVersion": "0.17.0"}));
        state.extend(trust.app_state_patch());
        assert_eq!(state["lastSeenCliVersion"], "0.17.0");
        assert!(state[EXTENSION_TRUST_STATE_KEY].is_object());
    }
}
