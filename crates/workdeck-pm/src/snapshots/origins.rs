//! Retained JSON artifacts are the durable source of historical declarations.
//! Validate identity/row links, while allowing later semantic edits to native
//! titles, bodies and other mutable fields without rewriting their origin.
use super::*;
use crate::{
    documents::{MAX_DOCUMENT_BYTES, MarkdownDocument},
    migration::MigrationKind,
    transactions::Snapshot,
};
use serde_json::Value;

pub(super) fn inspect(
    root: &Path,
    snapshot: &Snapshot<'_>,
    exports: &BTreeMap<PathBuf, LegacyExport>,
    errors: &mut Vec<PmError>,
) {
    for prefix in ["issues", "imported-sessions"] {
        let paths = match snapshot.list_bounded(Path::new(prefix), MAX_SNAPSHOT_ENTRIES) {
            Ok(paths) => paths,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        for path in paths {
            if (prefix == "issues"
                && (path.components().count() != 3
                    || path.file_name().is_none_or(|name| name != "item.md")))
                || (prefix == "imported-sessions"
                    && (path.components().count() != 2
                        || path.extension().is_none_or(|ext| ext != "toml")))
            {
                continue;
            }
            let absolute = root.join(&path);
            // The ordinary doctor already reports malformed record syntax.
            let Ok(Some(bytes)) = snapshot.read_bounded(&path, MAX_DOCUMENT_BYTES) else {
                continue;
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue;
            };
            if prefix == "issues" {
                let Ok(document) = MarkdownDocument::parse(&absolute, text) else {
                    continue;
                };
                let Ok(issue) = crate::issues::parse_issue_metadata(&absolute, &document) else {
                    continue;
                };
                let typed = issue
                    .imported_completion
                    .as_ref()
                    .filter(|origin| origin.source_path.starts_with("imported-history/exports/"))
                    .map(|origin| (origin.source_path.as_str(), &origin.source_content));
                if let Err(error) = check(
                    MigrationKind::Issue,
                    issue.id.as_str(),
                    issue.custom.get("legacy"),
                    typed,
                    exports,
                ) {
                    errors.push(error.at(&absolute));
                }
            } else {
                let Ok(session) = toml::from_str::<crate::RecordedSession>(text) else {
                    continue;
                };
                let origin = session
                    .extra
                    .get("x-workdeck-json-origin")
                    .and_then(|value| serde_json::to_value(value).ok());
                if let Err(error) = check(
                    MigrationKind::ImportedSession,
                    &session.id,
                    origin.as_ref(),
                    None,
                    exports,
                ) {
                    errors.push(error.at(&absolute));
                }
            }
        }
    }
    match snapshot.list_bounded(Path::new("operations"), MAX_SNAPSHOT_ENTRIES) {
        Err(error) => errors.push(error),
        Ok(paths) => {
            for path in paths {
                let Ok(Some(bytes)) = snapshot.read_bounded(&path, MAX_SNAPSHOT_CONTENT_BYTES)
                else {
                    continue;
                };
                let Ok(receipt) =
                    serde_yaml_ng::from_slice::<crate::transactions::MutationReceipt>(&bytes)
                else {
                    continue;
                };
                if receipt.operation != "snapshot.import_legacy" {
                    continue;
                }
                let checked: Result<()> = (|| {
                    let source = receipt
                        .result
                        .get("source_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            invalid("legacy import receipt requires its retained export source")
                        })?;
                    let input = exports.get(Path::new(source)).ok_or_else(|| {
                        invalid(
                            "legacy import receipt is missing its valid retained export artifact",
                        )
                    })?;
                    let repository =
                        crate::repository::config_from_snapshot(root, snapshot)?.repository;
                    super::legacy_import::validate_legacy_receipt(&receipt, input, &repository)?;
                    Ok(())
                })();
                if let Err(error) = checked {
                    errors.push(error.at(root.join(path)));
                }
            }
        }
    }
    for (kind, migration_kind) in [
        (crate::PlanningKind::Project, MigrationKind::Project),
        (crate::PlanningKind::Cycle, MigrationKind::Cycle),
        (crate::PlanningKind::Label, MigrationKind::Labels),
    ] {
        let Ok(records) = crate::planning::store::list_planning(root, snapshot, kind) else {
            continue;
        };
        for record in records {
            let typed = record
                .metadata
                .imported
                .as_ref()
                .filter(|origin| origin.format == crate::PlanningImportFormat::LegacyJson)
                .map(|origin| (origin.source.as_str(), &origin.content));
            if let Err(error) = check(
                migration_kind.clone(),
                &record.metadata.id,
                record.metadata.custom.get("legacy"),
                typed,
                exports,
            ) {
                errors.push(error.at(root.join(record.path)));
            }
        }
    }
}
fn check(
    kind: MigrationKind,
    id: &str,
    origin: Option<&Value>,
    typed: Option<(&str, &ContentHash)>,
    exports: &BTreeMap<PathBuf, LegacyExport>,
) -> Result<()> {
    let json_origin = origin
        .filter(|origin| origin.get("format").and_then(Value::as_str) == Some("workdeck_json_v0"));
    let Some(origin) = json_origin else {
        if typed.is_some() {
            return Err(invalid(
                "JSON import provenance requires its retained export and row identity",
            ));
        }
        return Ok(());
    };
    let source = origin
        .get("source_path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("JSON import provenance requires a retained export path"))?;
    let content: ContentHash =
        serde_json::from_value(origin.get("source_content").cloned().unwrap_or(Value::Null))
            .map_err(|_| invalid("JSON import provenance requires its retained export hash"))?;
    let selector = origin
        .get("record_selector")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            invalid("JSON import provenance requires its retained export row selector")
        })?;
    if typed.is_some_and(|(path, hash)| path != source || hash != &content) {
        return Err(invalid(
            "typed JSON provenance disagrees with its retained export identity",
        ));
    }
    let export = exports.get(Path::new(source)).ok_or_else(|| {
        invalid("JSON import provenance is missing its valid retained export artifact")
    })?;
    let key = if kind == MigrationKind::Issue {
        "key"
    } else {
        "id"
    };
    if content != export.hash
        || !export.rows.iter().any(|row| {
            row.kind == kind
                && row.selector == selector
                && row.value.get(key).and_then(Value::as_str) == Some(id)
        })
    {
        return Err(invalid(
            "JSON import provenance does not identify the same record in its retained export",
        ));
    }
    Ok(())
}
