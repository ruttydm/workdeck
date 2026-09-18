//! Ordered daemon mirror of the publication each live session currently serves.

use serde_json::Value;
use workdeck_review::{
    ReviewPublicationAddress, ReviewPublicationOrder, classify_review_publication,
};

use crate::{
    WorkdeckReviewResourceCatalogV1, parse_workdeck_review_publication_address,
    parse_workdeck_review_resource_catalog,
};

/// One session's mirrored position and the resources offered at that generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirroredReviewPublication {
    pub address: ReviewPublicationAddress,
    pub catalog: WorkdeckReviewResourceCatalogV1,
}

/// The effect one observed publication had on the mirror.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewMirrorUpdate {
    Adopted {
        generation: String,
    },
    Advanced {
        generation: String,
        state_revision: u64,
    },
    Replaced {
        generation: String,
        previous_generation: String,
    },
    Ignored,
}

/// One registration catalog and snapshot address observed together for a session.
#[derive(Debug, Clone, Copy)]
pub struct ObserveReviewPublicationInput<'a> {
    pub session_id: &'a str,
    pub catalog: Option<&'a WorkdeckReviewResourceCatalogV1>,
    pub address: Option<&'a ReviewPublicationAddress>,
}

/// Keeps only publication positions and catalogs; review content remains producer-owned.
#[derive(Debug, Default)]
pub struct ReviewMirror {
    publications: Vec<(String, MirroredReviewPublication)>,
}

impl ReviewMirror {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What the daemon currently believes one session is serving.
    #[must_use]
    pub fn get(&self, session_id: &str) -> Option<&MirroredReviewPublication> {
        self.publications
            .iter()
            .find_map(|(id, publication)| (id == session_id).then_some(publication))
    }

    /// Every mirrored session in first-adoption order.
    #[must_use]
    pub fn session_ids(&self) -> Vec<String> {
        self.publications
            .iter()
            .map(|(session_id, _)| session_id.clone())
            .collect()
    }

    /// Order one reported publication against the session's current mirrored position.
    pub fn observe(&mut self, input: ObserveReviewPublicationInput<'_>) -> ReviewMirrorUpdate {
        let Some(address) = input.address else {
            return ReviewMirrorUpdate::Ignored;
        };
        let matching_catalog = input
            .catalog
            .filter(|catalog| catalog.generation == address.generation);
        let current_index = self
            .publications
            .iter()
            .position(|(session_id, _)| session_id == input.session_id);
        let Some(current_index) = current_index else {
            let Some(catalog) = matching_catalog else {
                return ReviewMirrorUpdate::Ignored;
            };
            self.publications.push((
                input.session_id.to_owned(),
                MirroredReviewPublication {
                    address: address.clone(),
                    catalog: catalog.clone(),
                },
            ));
            return ReviewMirrorUpdate::Adopted {
                generation: address.generation.clone(),
            };
        };

        let current = &mut self.publications[current_index].1;
        match classify_review_publication(&current.address, address) {
            ReviewPublicationOrder::Accepted => {
                current.address = address.clone();
                ReviewMirrorUpdate::Advanced {
                    generation: address.generation.clone(),
                    state_revision: address.state_revision,
                }
            }
            ReviewPublicationOrder::Gap => {
                let Some(catalog) = matching_catalog else {
                    return ReviewMirrorUpdate::Ignored;
                };
                let previous_generation = current.address.generation.clone();
                *current = MirroredReviewPublication {
                    address: address.clone(),
                    catalog: catalog.clone(),
                };
                ReviewMirrorUpdate::Replaced {
                    generation: address.generation.clone(),
                    previous_generation,
                }
            }
            ReviewPublicationOrder::Stale => ReviewMirrorUpdate::Ignored,
        }
    }

    /// Forget a disconnected or pruned session's publication.
    pub fn forget(&mut self, session_id: &str) {
        self.publications
            .retain(|(candidate, _)| candidate != session_id);
    }

    /// Forget every publication while the daemon shuts down.
    pub fn clear(&mut self) {
        self.publications.clear();
    }
}

/// Read and validate the review catalog from an otherwise opaque registration payload.
#[must_use]
pub fn read_registration_review_catalog(value: &Value) -> Option<WorkdeckReviewResourceCatalogV1> {
    value
        .as_object()?
        .get("info")?
        .as_object()?
        .get("reviewCatalog")
        .and_then(parse_workdeck_review_resource_catalog)
}

/// Read and validate the publication address from an otherwise opaque snapshot payload.
#[must_use]
pub fn read_snapshot_review_publication(value: &Value) -> Option<ReviewPublicationAddress> {
    value
        .as_object()?
        .get("state")?
        .as_object()?
        .get("reviewPublication")
        .and_then(parse_workdeck_review_publication_address)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use workdeck_review::{
        REVIEW_PATCH_CONTENT_TYPE, ReviewResourceDescriptor, ReviewResourceDescriptorBase,
    };

    use super::*;

    const FILE_KEY: &str = "file:0123456789abcdef";

    fn catalog(generation: &str) -> WorkdeckReviewResourceCatalogV1 {
        WorkdeckReviewResourceCatalogV1 {
            generation: generation.into(),
            file_keys_by_runtime_id: BTreeMap::from([("file-1".into(), FILE_KEY.into())]),
            resources: vec![ReviewResourceDescriptor::Patch {
                descriptor: ReviewResourceDescriptorBase {
                    id: format!("resource:patch:{FILE_KEY}"),
                    generation: generation.into(),
                    file_key: FILE_KEY.into(),
                    byte_length: None,
                    digest: None,
                },
                content_type: REVIEW_PATCH_CONTENT_TYPE.into(),
            }],
        }
    }

    fn address(generation: &str, state_revision: u64) -> ReviewPublicationAddress {
        ReviewPublicationAddress {
            generation: generation.into(),
            state_revision,
        }
    }

    fn observe<'a>(
        mirror: &mut ReviewMirror,
        session_id: &'a str,
        catalog: Option<&'a WorkdeckReviewResourceCatalogV1>,
        address: Option<&'a ReviewPublicationAddress>,
    ) -> ReviewMirrorUpdate {
        mirror.observe(ObserveReviewPublicationInput {
            session_id,
            catalog,
            address,
        })
    }

    #[test]
    fn adopts_a_sessions_first_publication() {
        let mut mirror = ReviewMirror::new();
        let catalog = catalog("generation:p1:0");
        let address = address("generation:p1:0", 0);
        assert_eq!(
            observe(&mut mirror, "s-1", Some(&catalog), Some(&address)),
            ReviewMirrorUpdate::Adopted {
                generation: "generation:p1:0".into()
            }
        );
        assert_eq!(mirror.get("s-1").unwrap().address.state_revision, 0);
    }

    #[test]
    fn advances_on_a_further_revision_contiguous_or_not() {
        let mut mirror = ReviewMirror::new();
        let catalog = catalog("generation:p1:0");
        let first = address("generation:p1:0", 1);
        observe(&mut mirror, "s-1", Some(&catalog), Some(&first));
        let later = address("generation:p1:0", 41);
        assert_eq!(
            observe(&mut mirror, "s-1", None, Some(&later)),
            ReviewMirrorUpdate::Advanced {
                generation: "generation:p1:0".into(),
                state_revision: 41,
            }
        );
        assert_eq!(mirror.get("s-1").unwrap().address.state_revision, 41);
    }

    #[test]
    fn ignores_replayed_or_earlier_revisions() {
        let mut mirror = ReviewMirror::new();
        let catalog = catalog("generation:p1:0");
        let first = address("generation:p1:0", 5);
        observe(&mut mirror, "s-1", Some(&catalog), Some(&first));
        for revision in [5, 4, 0] {
            let replay = address("generation:p1:0", revision);
            assert_eq!(
                observe(&mut mirror, "s-1", None, Some(&replay)),
                ReviewMirrorUpdate::Ignored
            );
        }
        assert_eq!(mirror.get("s-1").unwrap().address.state_revision, 5);
    }

    #[test]
    fn replaces_the_publication_when_generation_advances() {
        let mut mirror = ReviewMirror::new();
        let old_catalog = catalog("generation:p1:0");
        let old_address = address("generation:p1:0", 9);
        observe(&mut mirror, "s-1", Some(&old_catalog), Some(&old_address));
        let next_catalog = catalog("generation:p1:1");
        let next_address = address("generation:p1:1", 0);
        assert_eq!(
            observe(&mut mirror, "s-1", Some(&next_catalog), Some(&next_address),),
            ReviewMirrorUpdate::Replaced {
                generation: "generation:p1:1".into(),
                previous_generation: "generation:p1:0".into(),
            }
        );
        assert_eq!(
            mirror.get("s-1").unwrap().catalog.generation,
            "generation:p1:1"
        );
    }

    #[test]
    fn waits_for_catalog_before_adopting_a_new_generation() {
        let mut mirror = ReviewMirror::new();
        let old_catalog = catalog("generation:p1:0");
        let old_address = address("generation:p1:0", 2);
        observe(&mut mirror, "s-1", Some(&old_catalog), Some(&old_address));
        let next_address = address("generation:p1:1", 0);
        assert_eq!(
            observe(&mut mirror, "s-1", Some(&old_catalog), Some(&next_address),),
            ReviewMirrorUpdate::Ignored
        );
        assert_eq!(
            mirror.get("s-1").unwrap().address.generation,
            "generation:p1:0"
        );
    }

    #[test]
    fn ignores_a_publication_from_an_unrelated_producer() {
        let mut mirror = ReviewMirror::new();
        let first_catalog = catalog("generation:p1:0");
        let first_address = address("generation:p1:0", 2);
        observe(
            &mut mirror,
            "s-1",
            Some(&first_catalog),
            Some(&first_address),
        );
        let unrelated_catalog = catalog("generation:p2:9");
        let unrelated_address = address("generation:p2:9", 900);
        assert_eq!(
            observe(
                &mut mirror,
                "s-1",
                Some(&unrelated_catalog),
                Some(&unrelated_address),
            ),
            ReviewMirrorUpdate::Ignored
        );
    }

    #[test]
    fn mirrors_nothing_for_a_session_that_publishes_nothing() {
        let mut mirror = ReviewMirror::new();
        assert_eq!(
            observe(&mut mirror, "s-1", None, None),
            ReviewMirrorUpdate::Ignored
        );
        assert_eq!(mirror.get("s-1"), None);
        assert!(mirror.session_ids().is_empty());
    }

    #[test]
    fn forgets_one_session_and_clears_them_all_in_insertion_order() {
        let mut mirror = ReviewMirror::new();
        let first_catalog = catalog("generation:p1:0");
        let first_address = address("generation:p1:0", 0);
        let second_catalog = catalog("generation:p2:0");
        let second_address = address("generation:p2:0", 0);
        observe(
            &mut mirror,
            "s-1",
            Some(&first_catalog),
            Some(&first_address),
        );
        observe(
            &mut mirror,
            "s-2",
            Some(&second_catalog),
            Some(&second_address),
        );
        assert_eq!(mirror.session_ids(), vec!["s-1", "s-2"]);
        mirror.forget("s-1");
        assert_eq!(mirror.session_ids(), vec!["s-2"]);
        mirror.clear();
        assert!(mirror.session_ids().is_empty());
    }

    fn catalog_value(generation: &str) -> Value {
        serde_json::to_value(catalog(generation)).unwrap()
    }

    #[test]
    fn payload_readers_use_the_shared_catalog_and_address_parsers() {
        let catalog = catalog_value("generation:p1:0");
        assert_eq!(
            read_registration_review_catalog(&json!({"info": {"reviewCatalog": catalog}})),
            Some(self::catalog("generation:p1:0"))
        );
        assert_eq!(
            read_snapshot_review_publication(&json!({
                "state": {"reviewPublication": {"generation": "generation:p1:0", "stateRevision": 3}}
            })),
            Some(address("generation:p1:0", 3))
        );
    }

    #[test]
    fn payload_readers_return_nothing_when_fields_are_absent() {
        assert_eq!(read_registration_review_catalog(&json!({"info": {}})), None);
        assert_eq!(read_registration_review_catalog(&Value::Null), None);
        assert_eq!(
            read_snapshot_review_publication(&json!({"state": {}})),
            None
        );
        assert_eq!(read_snapshot_review_publication(&Value::Null), None);
    }

    #[test]
    fn payload_readers_return_nothing_for_malformed_values() {
        assert_eq!(
            read_registration_review_catalog(
                &json!({"info": {"reviewCatalog": {"generation": 1}}})
            ),
            None
        );
        assert_eq!(
            read_snapshot_review_publication(
                &json!({"state": {"reviewPublication": {"generation": "x"}}})
            ),
            None
        );
    }
}
