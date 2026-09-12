use super::*;
use crate::transactions::{Snapshot, canonical_hash};
use serde_json::json;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(super) struct Capture {
    pub root: PathBuf,
    pub config: Config,
    pub graph: IssueGraphSnapshot,
    pub query: IssueQuerySnapshot,
    pub questions: Vec<QuestionRecord>,
}
impl Capture {
    pub fn load(root: &Path, snapshot: &Snapshot<'_>, config: Config) -> Result<Self> {
        let graph = crate::graph::capture(root, snapshot, &config)?;
        let query = crate::queries::capture(root, snapshot)?;
        let questions = crate::questions::load_questions(root, snapshot, &config)?;
        Ok(Self {
            root: root.to_owned(),
            config,
            graph,
            query,
            questions,
        })
    }
    pub fn resolve(&self, reference: &str) -> Result<&IssueRecord> {
        let value = if reference.contains("::") {
            let qualified: QualifiedRef = reference.parse()?;
            if qualified.repository != self.config.repository {
                return Err(PmError::new(
                    ErrorCode::NotFound,
                    "issue belongs to another repository",
                ));
            }
            qualified.record.to_string()
        } else {
            reference.to_owned()
        };
        if value.len() < 4
            || !value
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "issue reference requires an ID or at least four unambiguous prefix characters",
            ));
        }
        if let Some(issue) = self
            .graph
            .issues()
            .iter()
            .find(|i| i.metadata.id.as_str() == value)
        {
            return Ok(issue);
        }
        let mut matches = self
            .graph
            .issues()
            .iter()
            .filter(|i| i.metadata.id.as_str().starts_with(&value));
        let issue = matches
            .next()
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "issue was not found"))?;
        if matches.next().is_some() {
            return Err(PmError::new(
                ErrorCode::AmbiguousReference,
                "issue prefix matches multiple records",
            ));
        }
        Ok(issue)
    }
    pub fn applicability(
        &self,
        root: &Path,
        snapshot: &Snapshot<'_>,
        issue: &IssueRecord,
    ) -> Result<Vec<QuestionApplicability>> {
        crate::questions::applicability(
            root,
            snapshot,
            &self.config,
            &self.questions,
            &subjects(issue),
        )
    }
    pub fn anchor(
        &self,
        snapshot: &Snapshot<'_>,
        issue: &IssueRecord,
        questions: &[QuestionApplicability],
        external: &super::external::External,
        runs: &super::check_runs::Runs,
        reviews: &super::reviews::Reviews,
    ) -> Result<ContextAnchor> {
        let config_pin = SourcePin {
            path: "config.yml".into(),
            content: ContentHash::of(&snapshot.read(Path::new("config.yml"))?.ok_or_else(
                || PmError::new(ErrorCode::CorruptStore, "configuration disappeared"),
            )?),
        };
        let source_pins = vec![
            config_pin,
            SourcePin {
                path: issue.path.clone(),
                content: issue.source.content.clone(),
            },
        ];
        let mut feature_sources = Vec::new();
        if !issue.metadata.features.is_empty() {
            for feature in crate::features::load_features(snapshot, &self.config)? {
                if issue.metadata.features.contains(&feature.metadata.id) {
                    feature_sources.push((feature.metadata.id.clone(), feature.source.clone()));
                }
            }
        }
        let requirements = super::requirements::fingerprint(&self.root, snapshot, self, issue)?;
        let evidence = super::packet::relevant_evidence(snapshot, &self.config, issue)?
            .into_iter()
            .map(|r| (r.reference.id, r.content))
            .collect::<Vec<_>>();
        let mut basis = json!({"schema":1,"repository":self.config.repository,"issue":issue.metadata.id,"source":issue.source,"requirements":requirements,"graph":self.graph.fingerprint(),"questions":questions,"source_pins":source_pins,"external":external.fingerprint()?,"features":feature_sources,"evidence":evidence});
        // Repositories without run history keep their existing anchor encoding.
        if runs.total > 0 {
            basis["check_runs"] = json!(runs.fingerprint);
        }
        if let Some(reviews) = &reviews.report {
            basis["contract_reviews"] = json!(reviews.fingerprint);
        }
        let fingerprint = canonical_hash(&basis)?;
        Ok(ContextAnchor {
            schema_version: SchemaVersion::CURRENT,
            repository: self.config.repository.clone(),
            issue: issue.metadata.id.clone(),
            issue_source: issue.source.clone(),
            requirements,
            source_pins,
            fingerprint,
        })
    }
    pub fn source_links(
        &self,
        snapshot: &Snapshot<'_>,
        issue: &IssueRecord,
    ) -> Result<Vec<SourceLink>> {
        let mut links = issue.metadata.files.clone();
        if !issue.metadata.features.is_empty() {
            for feature in crate::features::load_features(snapshot, &self.config)? {
                if issue.metadata.features.contains(&feature.metadata.id) {
                    links.extend(feature.metadata.sources);
                }
            }
        }
        links.sort_by(|a, b| (&a.path, a.line, a.end_line).cmp(&(&b.path, b.line, b.end_line)));
        links.dedup();
        Ok(links)
    }
}
pub(crate) fn subjects(issue: &IssueRecord) -> Vec<SubjectRef> {
    let mut subjects = BTreeSet::from([SubjectRef::Issue(issue.metadata.id.clone())]);
    subjects.extend(
        issue
            .metadata
            .features
            .iter()
            .cloned()
            .map(SubjectRef::Feature),
    );
    subjects.extend(issue.metadata.gates.iter().cloned().map(SubjectRef::Gate));
    if let Some(id) = &issue.metadata.project {
        subjects.insert(SubjectRef::Project(id.clone()));
    }
    if let Some(id) = &issue.metadata.milestone {
        subjects.insert(SubjectRef::Milestone(id.clone()));
    }
    subjects.into_iter().collect()
}
pub(crate) fn anchor_for_issue(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    issue: &IssueId,
) -> Result<ContextAnchor> {
    let captured = Capture::load(root, snapshot, config.clone())?;
    let record = captured.resolve(issue.as_str())?;
    let questions = captured.applicability(root, snapshot, record)?;
    let links = captured.source_links(snapshot, record)?;
    let mut external = super::external::External::new(root)?;
    external.sources(&links)?;
    external.instructions(&links)?;
    let runs = super::check_runs::Runs::capture(root, snapshot, config, issue)?;
    let reviews = super::reviews::Reviews::capture(root, snapshot, config, record)?;
    let anchor = captured.anchor(snapshot, record, &questions, &external, &runs, &reviews)?;
    external.verify()?;
    runs.verify(root, snapshot, config, issue)?;
    reviews.verify(root, snapshot, config, record)?;
    Ok(anchor)
}
