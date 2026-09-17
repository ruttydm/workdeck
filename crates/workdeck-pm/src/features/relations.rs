use super::*;
use crate::{
    Config, Repository, RequestId, RetirementKind, RetirementTarget,
    transactions::{
        ChangedPath, FileChange, MutationReceipt, PreparedOperation, Snapshot, canonical_hash,
    },
};
use serde_json::json;
use std::path::Path;
const MAX_LINK_BYTES: usize = 4096;

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelatedFeatureLink {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub features: [FeatureId; 2],
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureRelatedInput {
    pub left: String,
    pub right: String,
    pub expected_left: Option<SourceToken>,
    pub expected_right: Option<SourceToken>,
    pub linked: bool,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureRelatedOutcome {
    pub input: FeatureRelatedInput,
    pub endpoints: [FeatureRecord; 2],
    pub before: Option<RelatedFeatureLink>,
    pub after: Option<RelatedFeatureLink>,
    pub path: PathBuf,
}
fn link(
    left: &FeatureId,
    right: &FeatureId,
    repository: &RepositoryId,
) -> Result<RelatedFeatureLink> {
    if left == right {
        return Err(invalid("a feature cannot be related to itself"));
    }
    let mut features = [left.clone(), right.clone()];
    features.sort();
    Ok(RelatedFeatureLink {
        schema: SchemaVersion::CURRENT,
        repository: repository.clone(),
        features,
    })
}
fn path(link: &RelatedFeatureLink) -> PathBuf {
    Path::new("relations/features")
        .join(link.features[0].as_str())
        .join(format!("{}.yml", link.features[1]))
}
pub(crate) fn validate_path(path: &Path) -> Result<()> {
    let parts = path
        .components()
        .map(|p| p.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| invalid("relation path must be UTF-8"))?;
    if parts.len() != 4 || parts[0] != "relations" || parts[1] != "features" {
        return Err(
            invalid("feature relation path must be relations/features/<low>/<high>.yml").at(path),
        );
    }
    let a: FeatureId = parts[2].parse()?;
    let b: FeatureId = parts[3]
        .strip_suffix(".yml")
        .ok_or_else(|| invalid("feature relation must be YAML"))?
        .parse()?;
    if a >= b {
        return Err(
            invalid("feature relation endpoints must use canonical sorted identities").at(path),
        );
    }
    Ok(())
}
fn bytes(link: &RelatedFeatureLink) -> Result<Vec<u8>> {
    serde_yaml_ng::to_string(link)
        .map(String::into_bytes)
        .map_err(|e| invalid(e.to_string()))
}
fn parse(path: &Path, source: &[u8], repository: &RepositoryId) -> Result<RelatedFeatureLink> {
    validate_path(path)?;
    if source.len() > MAX_LINK_BYTES {
        return Err(invalid("feature relation exceeds 4 KiB"));
    }
    let text =
        std::str::from_utf8(source).map_err(|_| invalid("feature relation must be UTF-8"))?;
    let document = crate::documents::YamlDocument::parse(path, text)?;
    let record: RelatedFeatureLink = document.deserialize()?;
    if &record.repository != repository || self::path(&record) != path || bytes(&record)? != source
    {
        return Err(invalid("feature relation differs from canonical source identity").at(path));
    }
    Ok(record)
}
pub(crate) fn load(snapshot: &Snapshot<'_>, config: &Config) -> Result<Vec<RelatedFeatureLink>> {
    let mut records = Vec::new();
    let mut total = 0;
    for path in snapshot.list_bounded(Path::new("relations/features"), 100_000)? {
        let source = snapshot
            .read_bounded(&path, MAX_LINK_BYTES)?
            .ok_or_else(|| invalid("feature relation disappeared"))?;
        total += source.len();
        if total > 32 * 1024 * 1024 {
            return Err(invalid("feature relations exceed 32 MiB"));
        }
        records.push(parse(&path, &source, &config.repository)?);
    }
    Ok(records)
}
impl Repository {
    pub fn set_feature_related(
        &self,
        left: &str,
        right: &str,
        expected_left: Option<&SourceToken>,
        expected_right: Option<&SourceToken>,
        linked: bool,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        let input = FeatureRelatedInput {
            left: left.into(),
            right: right.into(),
            expected_left: expected_left.cloned(),
            expected_right: expected_right.cloned(),
            linked,
        };
        let receipt =
            self.store()?
                .transact(request, "feature.related", &json!(input), |snapshot| {
                    let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
                    let records = store::qualified(self.root(), snapshot, &config)?;
                    let a = store::resolve(&records, left)?.clone();
                    let b = store::resolve(&records, right)?.clone();
                    for (record, expected) in [(&a, expected_left), (&b, expected_right)] {
                        if expected.is_some_and(|token| token != &record.source) {
                            return Err(PmError::new(
                                ErrorCode::StaleSource,
                                "feature relation endpoint changed since inspection",
                            )
                            .at(&record.path));
                        }
                        crate::retirement::ensure_writable(
                            self.root(),
                            snapshot,
                            &config,
                            &RetirementTarget::new(
                                RetirementKind::Feature,
                                record.metadata.id.as_str(),
                            )?,
                        )?;
                    }
                    let association = link(&a.metadata.id, &b.metadata.id, &config.repository)?;
                    let path = path(&association);
                    let old = snapshot.read_bounded(&path, MAX_LINK_BYTES)?;
                    let before = old
                        .as_deref()
                        .map(|bytes| parse(&path, bytes, &config.repository))
                        .transpose()?;
                    let after = linked.then_some(association);
                    let content = after.as_ref().map(bytes).transpose()?;
                    let changes = if old == content {
                        vec![]
                    } else {
                        vec![FileChange {
                            path: path.clone(),
                            expected: old.as_deref().map(ContentHash::of),
                            content,
                        }]
                    };
                    let result = json!(FeatureRelatedOutcome {
                        input: input.clone(),
                        endpoints: [a, b],
                        before,
                        after,
                        path
                    });
                    validate_result(
                        &config.repository,
                        &canonical_hash(&json!(input))?,
                        &result,
                        &proof::changed_paths(&changes),
                    )?;
                    Ok(PreparedOperation { changes, result })
                })?;
        validate_receipt(&receipt)?;
        Ok(receipt)
    }
}
pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if receipt.operation != "feature.related" {
        return Ok(());
    }
    validate_result(
        receipt.repository.as_ref().ok_or_else(corrupt)?,
        &receipt.input_hash,
        &receipt.result,
        &receipt.changed,
    )
    .map_err(|e| PmError::new(ErrorCode::CorruptStore, e.message))
}
fn validate_result(
    repository: &RepositoryId,
    input_hash: &ContentHash,
    result: &Value,
    changed: &[ChangedPath],
) -> Result<()> {
    let outcome: FeatureRelatedOutcome =
        serde_json::from_value(result.clone()).map_err(|_| corrupt())?;
    if canonical_hash(&json!(outcome.input))? != *input_hash {
        return Err(corrupt());
    }
    for (record, reference, expected) in [
        (
            &outcome.endpoints[0],
            &outcome.input.left,
            &outcome.input.expected_left,
        ),
        (
            &outcome.endpoints[1],
            &outcome.input.right,
            &outcome.input.expected_right,
        ),
    ] {
        validate_record(record, repository)?;
        if record.retirement.is_some()
            || !store::matches_reference(&record.metadata.id, reference)
            || expected
                .as_ref()
                .is_some_and(|token| token != &record.source)
        {
            return Err(corrupt());
        }
    }
    let association = link(
        &outcome.endpoints[0].metadata.id,
        &outcome.endpoints[1].metadata.id,
        repository,
    )?;
    if outcome.path != path(&association)
        || outcome
            .before
            .as_ref()
            .is_some_and(|before| before != &association)
        || outcome.after != outcome.input.linked.then_some(association)
    {
        return Err(corrupt());
    }
    let before = outcome.before.as_ref().map(bytes).transpose()?;
    let after = outcome.after.as_ref().map(bytes).transpose()?;
    let expected = if before == after {
        vec![]
    } else {
        vec![ChangedPath {
            path: outcome.path,
            before: before.as_deref().map(ContentHash::of),
            after: after.as_deref().map(ContentHash::of),
        }]
    };
    if expected != changed {
        return Err(corrupt());
    }
    Ok(())
}
pub(crate) fn incoming(
    snapshot: &Snapshot<'_>,
    config: &Config,
    target: &RetirementTarget,
    records: &[FeatureRecord],
) -> Result<Vec<crate::RecordReferenceBlocker>> {
    if target.kind != RetirementKind::Feature {
        return Ok(vec![]);
    }
    let id: FeatureId = target.id.parse()?;
    let mut blockers = vec![];
    for association in load(snapshot, config)? {
        if !association.features.contains(&id) {
            continue;
        }
        let other = association
            .features
            .iter()
            .find(|other| *other != &id)
            .expect("distinct endpoints");
        if records.iter().any(|record| &record.metadata.id == other) {
            blockers.push(crate::RecordReferenceBlocker {
                kind: RetirementKind::Feature,
                id: other.to_string(),
                path: path(&association),
                field: "related".into(),
                source: SourceToken {
                    revision: Revision::INITIAL,
                    content: ContentHash::of(&bytes(&association)?),
                },
            });
        } else {
            return Err(invalid("feature relation has an unresolved endpoint"));
        }
    }
    Ok(blockers)
}
fn corrupt() -> PmError {
    PmError::new(
        ErrorCode::CorruptStore,
        "feature relation source/result/input proof is inconsistent",
    )
}

pub(crate) fn validate_import(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    path: &Path,
) -> Result<()> {
    let source = snapshot
        .read_bounded(path, MAX_LINK_BYTES)?
        .ok_or_else(|| invalid("imported feature relation disappeared"))?;
    let link = parse(path, &source, &config.repository)?;
    let records = store::qualified(root, snapshot, config)?;
    for id in link.features {
        if !records.iter().any(|record| record.metadata.id == id) {
            return Err(PmError::new(
                ErrorCode::NotFound,
                format!("new feature relation endpoint {id} must resolve"),
            ));
        }
        crate::retirement::ensure_writable(
            root,
            snapshot,
            config,
            &RetirementTarget::new(RetirementKind::Feature, id.as_str())?,
        )?;
    }
    Ok(())
}
