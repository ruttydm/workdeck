//! Organization authority participates in independently captured CI contracts.
use crate::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiOrganizationContract {
    pub users: UsersRecord,
    pub schema: OrganizationSchemaRecord,
}

pub(crate) fn semantic_records(
    contract: &CiOrganizationContract,
) -> Result<BTreeMap<PathBuf, ContentHash>> {
    // Virtual default records ensure removal and introduction both compare.
    // Exact source tokens separately retain cosmetic edits and revisions.
    let registry = &contract.users.registry;
    let users: BTreeMap<_, _> = registry
        .users
        .iter()
        .map(|(id, user)| {
            (
                id,
                serde_json::json!({
                    "kind":user.kind, "archived":user.archived,
                    "custom":user.custom, "extra":user.extra,
                }),
            )
        })
        .collect();
    let schema = &contract.schema.definition;
    Ok(BTreeMap::from([
        (
            PathBuf::from("users.yml"),
            crate::transactions::canonical_hash(&serde_json::json!({
                "mode":registry.mode, "users":users,
                "custom":registry.custom, "extra":registry.extra,
            }))?,
        ),
        (
            PathBuf::from("schema.yml"),
            crate::transactions::canonical_hash(&serde_json::json!({
                "fields":schema.fields, "unit_mode":schema.unit_mode, "units":schema.units,
                "custom":schema.custom, "extra":schema.extra,
            }))?,
        ),
    ]))
}
