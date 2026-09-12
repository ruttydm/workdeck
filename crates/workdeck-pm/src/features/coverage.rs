use super::*;
use crate::{
    Config, RecordReferenceBlocker, Repository, RetirementKind, RetirementTarget,
    transactions::{Snapshot, canonical_hash},
};
use serde_json::json;
use std::path::Path;

impl Repository {
    pub fn feature_coverage(&self, query: &FeatureCoverageQuery) -> Result<FeatureCoverage> {
        self.store()?.with_snapshot(|snapshot|{
            let config=crate::repository::config_from_snapshot(self.root(),snapshot)?;
            let records=store::qualified(self.root(),snapshot,&config)?;
            let feature=store::resolve(&records,&query.feature)?.clone();
            let source=crate::queries::capture(self.root(),snapshot)?;
            // Capture the complete graph before applying the coverage filter.
            let members=source.issues().iter().filter(|issue|issue.metadata.features.contains(&feature.metadata.id)).map(|issue|issue.metadata.id.clone()).collect::<BTreeSet<_>>();
            let issues=source.select_indices(&query.issues)?.into_iter().map(|i|&source.issues()[i]).filter(|issue|members.contains(&issue.metadata.id)).cloned().collect::<Vec<_>>();
            let displayed=issues.iter().map(|issue|&issue.metadata.id).collect::<BTreeSet<_>>();
            let mut prerequisite_ids=BTreeSet::new();
            let mut pending=source.issues().iter().filter(|issue|members.contains(&issue.metadata.id)).collect::<Vec<_>>();
            let index=source.issues().iter().map(|issue|(&issue.metadata.id,issue)).collect::<BTreeMap<_,_>>();
            let mut visited=BTreeSet::new();
            let mut unresolved=Vec::new();
            while let Some(issue)=pending.pop() {
                if !visited.insert(&issue.metadata.id) { continue; }
                for id in &issue.metadata.prerequisites {
                    prerequisite_ids.insert(id);
                    if let Some(prerequisite)=index.get(id) {
                        pending.push(prerequisite);
                    } else {
                        unresolved.push(PmError::new(ErrorCode::NotFound,
                            format!("issue {} prerequisite {id} does not resolve; the retained declaration remains unresolved", issue.metadata.id))
                            .at(&issue.path)
                            .details(json!({"feature":feature.metadata.id,"issue":issue.metadata.id,"prerequisite":id,"source":issue.source,"reason":"prerequisite_missing"}))
                            .hint("Restore the prerequisite source or explicitly repair the authored prerequisite declaration."));
                    }
                }
            }
            // A same-feature prerequisite remains context when the issue filter
            // hides it. Only records already displayed are removed from context.
            let outside_prerequisites=source.issues().iter().filter(|issue|prerequisite_ids.contains(&issue.metadata.id)&&!displayed.contains(&issue.metadata.id)).cloned().collect();
            let links=relations::load(snapshot,&config)?;
            let related_ids=links.iter().filter(|link|link.features.contains(&feature.metadata.id)).flat_map(|link|link.features.iter().cloned()).collect::<BTreeSet<_>>();
            let children=records.iter().filter(|r|r.metadata.parent.as_ref()==Some(&feature.metadata.id)).cloned().collect();
            let prerequisites=records.iter().filter(|r|feature.metadata.prerequisites.contains(&r.metadata.id)).cloned().collect();
            let dependents=records.iter().filter(|r|r.metadata.prerequisites.contains(&feature.metadata.id)).cloned().collect();
            let related=records.iter().filter(|r|r.metadata.id!=feature.metadata.id&&related_ids.contains(&r.metadata.id)).cloned().collect();
            let config_hash=ContentHash::of(&snapshot.read(Path::new("config.yml"))?.ok_or_else(||invalid("configuration disappeared"))?);
            let mut warnings=diagnostics(self.root(),snapshot,&config,&records)?;
            warnings.extend(unresolved);
            let fingerprint=canonical_hash(&json!({"config":config_hash,"features":records.iter().map(|r|json!({"id":r.metadata.id,"path":r.path,"source":r.source,"retirement":r.retirement})).collect::<Vec<_>>(),"issues":source.issues().iter().map(|r|json!({"id":r.metadata.id,"source":r.source,"retirement":r.retirement})).collect::<Vec<_>>(),"related":links,"query":query}))?;
            Ok(FeatureCoverage{schema:SchemaVersion::CURRENT,repository:config.repository,query:query.clone(),feature,issues,outside_prerequisites,children,prerequisites,dependents,related,warnings,config:config_hash,fingerprint})
        })
    }
}

pub(crate) fn incoming(
    snapshot: &Snapshot<'_>,
    config: &Config,
    target: &RetirementTarget,
) -> Result<Vec<RecordReferenceBlocker>> {
    let records = load_features(snapshot, config)?;
    let mut blockers = Vec::new();
    for record in &records {
        let metadata = &record.metadata;
        let mut fields = Vec::new();
        match target.kind {
            RetirementKind::Feature => {
                if metadata
                    .parent
                    .as_ref()
                    .is_some_and(|id| id.as_str() == target.id)
                {
                    fields.push("parent");
                }
                if metadata
                    .prerequisites
                    .iter()
                    .any(|id| id.as_str() == target.id)
                {
                    fields.push("prerequisites");
                }
            }
            RetirementKind::Gate if metadata.gates.iter().any(|id| id.as_str() == target.id) => {
                fields.push("gates")
            }
            RetirementKind::Project
                if metadata
                    .projects
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(&target.id)) =>
            {
                fields.push("projects")
            }
            RetirementKind::Milestone
                if metadata
                    .milestones
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(&target.id)) =>
            {
                fields.push("milestones")
            }
            RetirementKind::Target
                if metadata
                    .targets
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(&target.id)) =>
            {
                fields.push("targets")
            }
            _ => {}
        }
        for field in fields {
            blockers.push(RecordReferenceBlocker {
                kind: RetirementKind::Feature,
                id: metadata.id.to_string(),
                path: record.path.clone(),
                field: field.into(),
                source: record.source.clone(),
            });
        }
    }
    blockers.extend(relations::incoming(snapshot, config, target, &records)?);
    Ok(blockers)
}
fn diagnostics(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    records: &[FeatureRecord],
) -> Result<Vec<PmError>> {
    let mut warnings = Vec::new();
    let index = policy::validation_index(records);
    for record in records {
        if let Err(error) = policy::validate_change_indexed(
            root,
            snapshot,
            config,
            &index,
            None,
            &record.metadata,
            true,
        ) {
            warnings.push(
                PmError::new(
                    error.code,
                    format!(
                        "feature {} declarations need repair: {}",
                        record.metadata.id, error.message
                    ),
                )
                .at(&record.path),
            );
        }
    }
    for link in relations::load(snapshot, config)? {
        for id in link.features {
            if !records.iter().any(|r| r.metadata.id == id) {
                warnings.push(PmError::new(
                    crate::ErrorCode::NotFound,
                    format!("related feature {id} is missing"),
                ));
            }
        }
    }
    let ids = records
        .iter()
        .map(|record| &record.metadata.id)
        .collect::<BTreeSet<_>>();
    for issue in crate::issues::load_issues(root, snapshot, config)? {
        for id in &issue.metadata.features {
            if !ids.contains(id) {
                warnings.push(
                    PmError::new(
                        crate::ErrorCode::NotFound,
                        format!("issue {} feature {id} is missing", issue.metadata.id),
                    )
                    .at(&issue.path),
                );
            }
        }
    }
    Ok(warnings)
}
pub(crate) fn inspect(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> (usize, Vec<PmError>, Vec<PmError>) {
    let records = match store::qualified(root, snapshot, config) {
        Ok(records) => records,
        Err(error) => return (0, vec![error], vec![]),
    };
    let mut errors = policy::graph_errors(&records);
    let count = records.len();
    let warnings = match diagnostics(root, snapshot, config, &records) {
        Ok(w) => w,
        Err(e) => {
            errors.push(e);
            vec![]
        }
    };
    if let Err(error) = relations::load(snapshot, config) {
        errors.push(error);
    }
    (count, errors, warnings)
}
pub(crate) fn validate_import(
    root: &Path,
    current: &Snapshot<'_>,
    projected: &Snapshot<'_>,
    config: &Config,
    path: &Path,
) -> Result<()> {
    let bytes = projected
        .read(path)?
        .ok_or_else(|| invalid("imported feature disappeared"))?;
    let next = parse(path, &bytes, &config.repository)?;
    let records = load_features(projected, config)?;
    let prior = load_features(current, config)?
        .into_iter()
        .find(|r| r.metadata.id == next.metadata.id);
    if let Some(old) = &prior {
        if old.metadata.created_at != next.metadata.created_at
            || old.metadata.repository != next.metadata.repository
        {
            return Err(invalid(
                "feature creation time and repository identity are immutable",
            ));
        }
        if (old.metadata != next.metadata || old.body != next.body || old.path != next.path)
            && (next.metadata.revision <= old.metadata.revision
                || next.metadata.updated_at < old.metadata.updated_at)
        {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "native feature replacement must advance revision and preserve timestamp ordering",
            ));
        }
    }
    crate::retirement::ensure_writable(
        root,
        current,
        config,
        &RetirementTarget::new(RetirementKind::Feature, next.metadata.id.as_str())?,
    )?;
    validate_change(
        root,
        projected,
        config,
        &records,
        prior.as_ref().map(|r| &r.metadata),
        &next.metadata,
        true,
    )
}
