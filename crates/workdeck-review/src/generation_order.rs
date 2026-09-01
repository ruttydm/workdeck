//! Total ordering contract for published review generations and revisions.

use thiserror::Error;

const GENERATION_PREFIX: &str = "generation";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewGenerationIdentity {
    pub producer_id: String,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewPublicationAddress {
    pub generation: String,
    pub state_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewPublicationOrder {
    Accepted,
    Stale,
    Gap,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReviewGenerationError {
    #[error("review producer id {0:?} is not addressable")]
    InvalidProducer(String),
    #[error("review generation sequence {0} is outside the interoperable integer range")]
    InvalidSequence(u64),
    #[error(
        "review publication {next_generation}@{next_revision} does not advance {current_generation}@{current_revision}"
    )]
    DoesNotAdvance {
        current_generation: String,
        current_revision: u64,
        next_generation: String,
        next_revision: u64,
    },
    #[error(
        "review generation {next} skips ahead of {current}; producers advance one generation at a time"
    )]
    SkippedGeneration { current: String, next: String },
}

pub fn format_review_generation(
    identity: &ReviewGenerationIdentity,
) -> Result<String, ReviewGenerationError> {
    if identity.producer_id.is_empty()
        || !identity
            .producer_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(ReviewGenerationError::InvalidProducer(
            identity.producer_id.clone(),
        ));
    }
    if identity.sequence > MAX_SAFE_INTEGER {
        return Err(ReviewGenerationError::InvalidSequence(identity.sequence));
    }
    Ok(format!(
        "{GENERATION_PREFIX}:{}:{}",
        identity.producer_id, identity.sequence
    ))
}

pub fn parse_review_generation(value: &str) -> Option<ReviewGenerationIdentity> {
    let mut fields = value.split(':');
    let prefix = fields.next()?;
    let producer_id = fields.next()?;
    let sequence = fields.next()?;
    if fields.next().is_some()
        || prefix != GENERATION_PREFIX
        || producer_id.is_empty()
        || !producer_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        || sequence.is_empty()
        || sequence.len() > 16
        || !sequence.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let sequence = sequence.parse::<u64>().ok()?;
    (sequence <= MAX_SAFE_INTEGER).then(|| ReviewGenerationIdentity {
        producer_id: producer_id.to_owned(),
        sequence,
    })
}

pub fn next_review_generation(identity: &ReviewGenerationIdentity) -> ReviewGenerationIdentity {
    ReviewGenerationIdentity {
        producer_id: identity.producer_id.clone(),
        sequence: identity.sequence.saturating_add(1),
    }
}

pub fn classify_review_publication(
    current: &ReviewPublicationAddress,
    incoming: &ReviewPublicationAddress,
) -> ReviewPublicationOrder {
    let Some(current_generation) = parse_review_generation(&current.generation) else {
        return ReviewPublicationOrder::Stale;
    };
    let Some(incoming_generation) = parse_review_generation(&incoming.generation) else {
        return ReviewPublicationOrder::Stale;
    };
    if current_generation.producer_id != incoming_generation.producer_id {
        return ReviewPublicationOrder::Stale;
    }
    if current_generation.sequence != incoming_generation.sequence {
        return if incoming_generation.sequence > current_generation.sequence {
            ReviewPublicationOrder::Gap
        } else {
            ReviewPublicationOrder::Stale
        };
    }
    if incoming.state_revision > current.state_revision {
        ReviewPublicationOrder::Accepted
    } else {
        ReviewPublicationOrder::Stale
    }
}

pub fn assert_review_publication_advance(
    current: &ReviewPublicationAddress,
    next: &ReviewPublicationAddress,
) -> Result<(), ReviewGenerationError> {
    match classify_review_publication(current, next) {
        ReviewPublicationOrder::Accepted => Ok(()),
        ReviewPublicationOrder::Stale => Err(ReviewGenerationError::DoesNotAdvance {
            current_generation: current.generation.clone(),
            current_revision: current.state_revision,
            next_generation: next.generation.clone(),
            next_revision: next.state_revision,
        }),
        ReviewPublicationOrder::Gap => {
            let current_generation = parse_review_generation(&current.generation)
                .expect("gap verdict requires valid generations");
            let next_generation = parse_review_generation(&next.generation)
                .expect("gap verdict requires valid generations");
            if next_generation.sequence != current_generation.sequence + 1 {
                return Err(ReviewGenerationError::SkippedGeneration {
                    current: current.generation.clone(),
                    next: next.generation.clone(),
                });
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation(producer: &str, sequence: u64) -> String {
        format_review_generation(&ReviewGenerationIdentity {
            producer_id: producer.into(),
            sequence,
        })
        .unwrap()
    }

    fn at(producer: &str, sequence: u64, state_revision: u64) -> ReviewPublicationAddress {
        ReviewPublicationAddress {
            generation: generation(producer, sequence),
            state_revision,
        }
    }

    #[test]
    fn generation_identity_round_trips_the_entire_safe_range() {
        for sequence in [
            0,
            999_999_999_999_999,
            1_000_000_000_000_000,
            MAX_SAFE_INTEGER,
        ] {
            let expected = ReviewGenerationIdentity {
                producer_id: "p1".into(),
                sequence,
            };
            assert_eq!(
                parse_review_generation(&format_review_generation(&expected).unwrap()),
                Some(expected)
            );
        }
        assert_eq!(
            parse_review_generation("generation:p1:9007199254740993"),
            None
        );
    }

    #[test]
    fn rejects_generation_values_outside_the_serialized_grammar() {
        for invalid in [
            "generation:p1",
            "generation::3",
            "generation:p 1:3",
            "gen:p1:3",
            "generation:p1:-1",
            "generation:p1:3.5",
        ] {
            assert_eq!(parse_review_generation(invalid), None);
        }
        assert!(
            format_review_generation(&ReviewGenerationIdentity {
                producer_id: "a:b".into(),
                sequence: 1,
            })
            .is_err()
        );
        assert_eq!(
            next_review_generation(&ReviewGenerationIdentity {
                producer_id: "p1".into(),
                sequence: 4,
            })
            .sequence,
            5
        );
    }

    #[test]
    fn classifies_revision_jumps_replays_generation_gaps_and_producers() {
        assert_eq!(
            classify_review_publication(&at("p1", 1, 3), &at("p1", 1, 4)),
            ReviewPublicationOrder::Accepted
        );
        assert_eq!(
            classify_review_publication(&at("p1", 1, 3), &at("p1", 1, 9)),
            ReviewPublicationOrder::Accepted
        );
        assert_eq!(
            classify_review_publication(&at("p1", 1, 3), &at("p1", 1, 3)),
            ReviewPublicationOrder::Stale
        );
        assert_eq!(
            classify_review_publication(&at("p1", 1, 9), &at("p1", 4, 0)),
            ReviewPublicationOrder::Gap
        );
        assert_eq!(
            classify_review_publication(&at("p1", 3, 0), &at("p1", 2, 99)),
            ReviewPublicationOrder::Stale
        );
        assert_eq!(
            classify_review_publication(&at("p1", 1, 0), &at("p2", 9, 9)),
            ReviewPublicationOrder::Stale
        );
        assert_eq!(
            classify_review_publication(
                &at("p1", 1, 0),
                &ReviewPublicationAddress {
                    generation: "nope".into(),
                    state_revision: 5,
                }
            ),
            ReviewPublicationOrder::Stale
        );
    }

    #[test]
    fn producer_assertion_accepts_only_valid_forward_shapes() {
        assert!(assert_review_publication_advance(&at("p1", 1, 2), &at("p1", 1, 3)).is_ok());
        assert!(assert_review_publication_advance(&at("p1", 1, 2), &at("p1", 2, 0)).is_ok());
        assert!(assert_review_publication_advance(&at("p1", 1, 2), &at("p1", 1, 2)).is_err());
        assert!(assert_review_publication_advance(&at("p1", 1, 2), &at("p1", 3, 0)).is_err());
    }
}
