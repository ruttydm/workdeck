use super::capture::{cycle, policy_hash, related_path, requirement_hash, waiver_path};
use super::*;
use crate::transactions::{FaultPoint, FileChange, MutationReceipt, PreparedOperation, Snapshot};
use std::{collections::BTreeMap, path::Path};

/// Prospective import is an authoring operation. Historical export/restoration
/// and byte-identical retained links continue to use structural validation.
pub(crate) fn validate_related_import(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    path: &Path,
) -> Result<()> {
    let graph = capture(root, snapshot, config)?;
    let relation = graph
        .related
        .iter()
        .find(|relation| {
            related_path(&relation.issues[0], &relation.issues[1])
                .is_ok_and(|candidate| candidate == path)
        })
        .ok_or_else(|| invalid("imported related issue link is absent").at(path))?;
    for id in &relation.issues {
        let issue = graph
            .issues
            .iter()
            .find(|issue| &issue.metadata.id == id)
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::NotFound,
                    format!("related issue {id} does not exist"),
                )
            })?;
        if issue.retirement.is_some() {
            return Err(blocked(format!(
                "related issue {id} is permanently retired"
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_issue_change(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    old: Option<&IssueMetadata>,
    next: &IssueMetadata,
) -> Result<()> {
    validate_metadata(next)?;
    let changed =
        old.is_none_or(|old| old.parent != next.parent || old.prerequisites != next.prerequisites);
    if !changed {
        return Ok(());
    }
    let mut graph = capture(root, snapshot, config)?;
    let index = graph.index();
    for (id, was) in next
        .parent
        .iter()
        .map(|id| (id, old.is_some_and(|o| o.parent.as_ref() == Some(id))))
        .chain(
            next.prerequisites
                .iter()
                .map(|id| (id, old.is_some_and(|o| o.prerequisites.contains(id)))),
        )
    {
        if was {
            continue;
        }
        let target = index.get(id).ok_or_else(|| {
            PmError::new(
                ErrorCode::NotFound,
                format!("graph reference {id} does not exist"),
            )
        })?;
        if target.retirement.is_some() {
            return Err(blocked(format!(
                "graph reference {id} is permanently retired"
            )));
        }
    }
    if config.acceptance.require_completed_children
        && old.and_then(|o| o.parent.as_ref()) != next.parent.as_ref()
    {
        for id in old
            .and_then(|o| o.parent.as_ref())
            .into_iter()
            .chain(next.parent.iter())
        {
            if let Some(parent) = index.get(id)
                && config.workflow.state(&parent.metadata.status)?.category
                    == WorkflowCategory::Completed
            {
                return Err(blocked(format!(
                    "reopen completed parent {id} before changing its required children"
                )));
            }
        }
    }
    drop(index);
    if let Some(record) = graph.issues.iter_mut().find(|i| i.metadata.id == next.id) {
        record.metadata = next.clone()
    } else {
        graph.issues.push(IssueRecord {
            metadata: next.clone(),
            body: String::new(),
            path: format!("issues/{}/item.md", next.id).into(),
            source: SourceToken {
                revision: next.revision,
                content: ContentHash::of(b"prospective"),
            },
            retirement: None,
        });
    }
    for (name, edges) in [
        (
            "parent",
            graph
                .issues
                .iter()
                .map(|i| {
                    (
                        i.metadata.id.clone(),
                        i.metadata.parent.iter().cloned().collect(),
                    )
                })
                .collect(),
        ),
        ("hard prerequisite", graph.hard_edges()),
        ("completion", graph.completion_edges()),
    ] {
        if let Some(path) = cycle(&edges) {
            return Err(blocked(format!(
                "{name} cycle: {}",
                path.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ))
            .details(json!({"path":path})));
        }
    }
    Ok(())
}
/// Changed requirements invalidate existing decisions in the same durable write.
pub(crate) fn invalidate_waivers(
    snapshot: &Snapshot<'_>,
    old: &IssueMetadata,
    next: &IssueMetadata,
) -> Result<Vec<FileChange>> {
    if old.prerequisites == next.prerequisites
        && old
            .acceptance
            .iter()
            .map(|a| (&a.id, &a.description))
            .eq(next.acceptance.iter().map(|a| (&a.id, &a.description)))
    {
        return Ok(Vec::new());
    }
    let mut changes = Vec::new();
    for path in snapshot.list_bounded(
        &PathBuf::from(format!("relations/waivers/{}", old.id)),
        MAX_ENTRIES,
    )? {
        validate_path(&path)?;
        let bytes = snapshot
            .read_bounded(&path, crate::documents::MAX_DOCUMENT_BYTES)?
            .ok_or_else(|| invalid("waiver disappeared"))?;
        changes.push(FileChange {
            path,
            expected: Some(ContentHash::of(&bytes)),
            content: None,
        });
    }
    Ok(changes)
}
impl Repository {
    pub fn mutate_issue_graph(
        &self,
        reference: &str,
        expected: Option<&SourceToken>,
        expected_graph: Option<&ContentHash>,
        mutation: &IssueGraphMutation,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.mutate_issue_graph_with_faults(
            reference,
            expected,
            expected_graph,
            mutation,
            request,
            |_| Ok(()),
        )
    }
    #[doc(hidden)]
    pub fn mutate_issue_graph_with_faults(
        &self,
        reference: &str,
        expected: Option<&SourceToken>,
        expected_graph: Option<&ContentHash>,
        mutation: &IssueGraphMutation,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let input = json!({"reference":reference,"expected":expected,"expected_graph":expected_graph,"mutation":mutation});
        let receipt = self.store()?.transact_with_faults(request,"issue.graph",&input,|s|{
            let config=crate::repository::config_from_snapshot(self.root(),s)?;let graph=capture(self.root(),s,&config)?;let issue=crate::issues::resolve_issue(self.root(),s,&config,reference)?;
            if expected.is_some_and(|v|v!=&issue.source){return Err(PmError::new(ErrorCode::StaleSource,"issue graph subject changed"));}
            crate::retirement::ensure_writable(self.root(),s,&config,&RetirementTarget::new(RetirementKind::Issue,issue.metadata.id.as_str())?)?;
            if expected_graph.is_some_and(|v|v!=&graph.fingerprint){return Err(PmError::new(ErrorCode::StaleSource,"reviewed graph source or membership changed"));}
            let resolve=|reference:&str|crate::issues::resolve_issue(self.root(),s,&config,reference);
            let resolve_existing=|reference:&str|->Result<IssueId>{if let Ok(id)=reference.parse::<IssueId>() && issue.metadata.prerequisites.contains(&id){return Ok(id)}Ok(resolve(reference)?.metadata.id)};
            let mut fields=BTreeMap::new();let mut additional=Vec::new();
            match mutation {
                IssueGraphMutation::SetParent{parent}=>{fields.insert("parent".into(),json!(parent.as_deref().map(resolve).transpose()?.map(|i|i.metadata.id)));},
                IssueGraphMutation::AddPrerequisite{prerequisite}=>{let id=resolve(prerequisite)?.metadata.id;let mut ids=issue.metadata.prerequisites.clone();if !ids.contains(&id){ids.push(id)}ids.sort();fields.insert("prerequisites".into(),json!(ids));},
                IssueGraphMutation::RemovePrerequisite{prerequisite,reason}|IssueGraphMutation::ReplacePrerequisite{prerequisite,reason,..}=>{
                    text_value(reason,"resolution reason")?;let id=resolve_existing(prerequisite)?;let mut ids=issue.metadata.prerequisites.clone();if !ids.contains(&id){return Err(PmError::new(ErrorCode::NotFound,"prerequisite is not present"));}ids.retain(|i|i!=&id);
                    if let IssueGraphMutation::ReplacePrerequisite{replacement,..}=mutation{let replacement=resolve(replacement)?.metadata.id;if replacement==id {return Err(PmError::new(ErrorCode::InvalidInput,"replacement must be a different prerequisite"));}
                    if !ids.contains(&replacement){ids.push(replacement)}}ids.sort();fields.insert("prerequisites".into(),json!(ids));
                },
                IssueGraphMutation::SetRelated{other,related}=>{
                    let other_id=if !related {other.parse::<IssueId>().or_else(|_|resolve(other).map(|i|i.metadata.id))?} else {let other=resolve(other)?;crate::retirement::ensure_writable(self.root(),s,&config,&RetirementTarget::new(RetirementKind::Issue,other.metadata.id.as_str())?)?;other.metadata.id};
                    let path=related_path(&issue.metadata.id,&other_id)?;let old=s.read_bounded(&path,crate::documents::MAX_DOCUMENT_BYTES)?;
                    if *related && old.is_none(){let mut ids=[issue.metadata.id.clone(),other_id];ids.sort();let link=RelatedIssueLink{schema:SchemaVersion::CURRENT,repository:config.repository.clone(),issues:ids,created_at:chrono::Utc::now()};additional.push(FileChange{path,expected:None,content:Some(serde_yaml_ng::to_string(&link).map_err(|e|invalid(e.to_string()))?.into_bytes())});}
                    else if !related && let Some(old)=old {additional.push(FileChange{path,expected:Some(ContentHash::of(&old)),content:None});}
                },
                IssueGraphMutation::WaivePrerequisite{prerequisite,actor,reason}=>{
                    text_value(actor,"waiver actor")?;text_value(reason,"waiver reason")?;
                    if !config.acceptance.allow_prerequisite_waivers {return Err(blocked("prerequisite waivers are disabled by acceptance policy"));}
                    if config.workflow.state(&issue.metadata.status)?.category==WorkflowCategory::Completed{return Err(blocked("reopen completed work before changing a waiver"));}
                    crate::organization::validate_actor(s,&config.repository,actor)?;
                    let required=resolve(prerequisite)?;
                    if !issue.metadata.prerequisites.contains(&required.metadata.id){return Err(PmError::new(ErrorCode::NotFound,"prerequisite is not present"));}
                    crate::retirement::ensure_writable(self.root(),s,&config,&RetirementTarget::new(RetirementKind::Issue,required.metadata.id.as_str())?)?;
                    let waiver=PrerequisiteWaiver{request_id:request.clone(),schema:SchemaVersion::CURRENT,repository:config.repository.clone(),issue:issue.metadata.id.clone(),prerequisite:required.metadata.id,requirement_hash:requirement_hash(&issue.metadata)?,prerequisite_source:required.source,policy_hash:policy_hash(&config)?,actor:actor.clone(),reason:reason.clone(),created_at:chrono::Utc::now()};
                    let path=waiver_path(&waiver.issue,&waiver.prerequisite);let before=s.read_bounded(&path,crate::documents::MAX_DOCUMENT_BYTES)?;additional.push(FileChange{path,expected:before.as_deref().map(ContentHash::of),content:Some(serde_yaml_ng::to_string(&waiver).map_err(|e|invalid(e.to_string()))?.into_bytes())});
                },
                IssueGraphMutation::RevokeWaiver{prerequisite,reason}=>{
                    text_value(reason,"revocation reason")?;if config.workflow.state(&issue.metadata.status)?.category==WorkflowCategory::Completed{return Err(blocked("reopen completed work before changing a waiver"));}
                    let id=resolve_existing(prerequisite)?;let path=waiver_path(&issue.metadata.id,&id);let old=s.read_bounded(&path,crate::documents::MAX_DOCUMENT_BYTES)?.ok_or_else(||PmError::new(ErrorCode::NotFound,"waiver is not present"))?;additional.push(FileChange{path,expected:Some(ContentHash::of(&old)),content:None});
                }
            }
            let mut prepared=if fields.is_empty(){PreparedOperation{changes:Vec::new(),result:json!(issue)}}else{crate::issues::prepare_issue_mutation(self.root(),s,&config,issue,expected,&IssueMutation::Update{input:UpdateIssue{fields,body:None}})?};
            let waivers=additional.iter().filter(|c|c.path.starts_with("relations/waivers")).filter_map(|c|c.content.as_deref()).map(|bytes|serde_yaml_ng::from_slice::<PrerequisiteWaiver>(bytes).map_err(|e|invalid(e.to_string()))).collect::<Result<Vec<_>>>()?;
            prepared.changes.extend(additional);
            prepared.result=json!({"issue":prepared.result,"mutation":mutation,"previous_graph":graph.fingerprint,"waivers":waivers,"intent":input});
            Ok(prepared)
        },fault)?;
        super::proof::validate_replay(&receipt, &input)?;
        Ok(receipt)
    }
}
