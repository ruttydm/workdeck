use workdeck_analysis::AnalyzedUnit;
use workdeck_domain::{
    AnalysisConfidence, RepositoryId, ReviewAnchor, ReviewDelta, ReviewUnitId, ReviewUnitKind,
    ReviewUnitVersion, ReviewUnitVersionId, SnapshotId, UnitTransition, classify_unit_transition,
    transition_carries_review_state,
};

#[derive(Debug, Clone)]
pub struct ReviewUnitInput {
    pub logical_key: String,
    pub repository_id: Option<RepositoryId>,
    pub path: Option<std::path::PathBuf>,
    pub qualified_name: Option<String>,
    pub kind: ReviewUnitKind,
    pub title: String,
    pub content: String,
    pub semantic_content: String,
    pub provenance: String,
    pub confidence: AnalysisConfidence,
}

#[derive(Default)]
pub struct ReviewEngine;

impl ReviewEngine {
    pub fn version_for(
        &self,
        snapshot_id: SnapshotId,
        unit_id: Option<ReviewUnitId>,
        input: &ReviewUnitInput,
    ) -> ReviewUnitVersion {
        ReviewUnitVersion {
            id: ReviewUnitVersionId::new(),
            unit_id: unit_id.unwrap_or_default(),
            snapshot_id,
            kind: input.kind,
            title: input.title.clone(),
            anchor: ReviewAnchor::for_text(
                input.repository_id.clone(),
                input.path.clone(),
                input.qualified_name.clone(),
                &input.content,
                &input.semantic_content,
            ),
            provenance: input.provenance.clone(),
            confidence: input.confidence,
        }
    }

    pub fn version_for_analyzed(
        &self,
        snapshot_id: SnapshotId,
        unit_id: Option<ReviewUnitId>,
        input: &AnalyzedUnit,
    ) -> ReviewUnitVersion {
        let mut version = self.version_for(
            snapshot_id,
            unit_id,
            &ReviewUnitInput {
                logical_key: input.logical_key.clone(),
                repository_id: input.repository_id.clone(),
                path: Some(input.path.clone()),
                qualified_name: input.qualified_name.clone(),
                kind: input.kind,
                title: input.title.clone(),
                content: input.content.clone(),
                semantic_content: input.semantic_content.clone(),
                provenance: input.provenance.clone(),
                confidence: input.confidence,
            },
        );
        version.anchor.start_byte = Some(input.start_byte);
        version.anchor.end_byte = Some(input.end_byte);
        version.anchor.start_line = Some(input.start_line);
        version.anchor.end_line = Some(input.end_line);
        version
    }

    pub fn compare(
        &self,
        previous: Option<&ReviewUnitVersion>,
        current: Option<&ReviewUnitVersion>,
    ) -> ReviewDelta {
        let transition = classify_unit_transition(previous, current);
        let unit_id = current
            .map(|version| version.unit_id.clone())
            .or_else(|| previous.map(|version| version.unit_id.clone()))
            .unwrap_or_default();
        ReviewDelta {
            unit_id,
            from_version: previous.map(|version| version.id.clone()),
            to_version: current.map(|version| version.id.clone()),
            transition,
            carry_review_state: transition_carries_review_state(transition),
            reason: transition_reason(transition).to_string(),
        }
    }
}

fn transition_reason(transition: UnitTransition) -> &'static str {
    match transition {
        UnitTransition::Unchanged => "content and logical anchor are unchanged",
        UnitTransition::Moved => "identical content moved to a new logical anchor",
        UnitTransition::Rebased => "equivalent content was rebased",
        UnitTransition::FormatOnly => "semantic content is unchanged",
        UnitTransition::Modified => "semantic content changed",
        UnitTransition::New => "unit is new in this checkpoint",
        UnitTransition::Removed => "unit was removed in this checkpoint",
        UnitTransition::DependencyImpact => "a related dependency changed",
        UnitTransition::Ambiguous => "the unit could not be re-anchored uniquely",
    }
}
