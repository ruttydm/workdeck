use super::*;
use crate::{ContentHash, RepositoryId, Revision, SchemaVersion, SourceToken};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    str::FromStr,
};

pub const MAX_ORGANIZATION_BYTES: usize = 512 * 1024;
pub(crate) const MAX_ORGANIZATION_ENTRIES: usize = 20_000;

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum IdentityMode {
    #[default]
    Open,
    Registered,
}
#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum UserKind {
    #[default]
    Human,
    Agent,
    Service,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserDefinition {
    pub name: String,
    #[serde(default)]
    pub kind: UserKind,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
impl UserDefinition {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: UserKind::Human,
            archived: false,
            custom: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsersRegistry {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub revision: Revision,
    #[serde(default)]
    pub mode: IdentityMode,
    #[serde(default)]
    pub users: BTreeMap<String, UserDefinition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsersRecord {
    pub registry: UsersRegistry,
    pub path: PathBuf,
    pub source: Option<SourceToken>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum UserMutation {
    Patch {
        name: Option<String>,
        kind: Option<UserKind>,
        #[serde(default)]
        custom: CustomPatch,
    },
    Create {
        user: UserDefinition,
    },
    Update {
        user: UserDefinition,
    },
    Archive {
        archived: bool,
    },
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CustomScope {
    Feature,
    Gate,
    Evidence,
    Issue,
    Initiative,
    Project,
    Milestone,
    Cycle,
    Target,
    Label,
}
impl From<crate::PlanningKind> for CustomScope {
    fn from(kind: crate::PlanningKind) -> Self {
        match kind {
            crate::PlanningKind::Initiative => Self::Initiative,
            crate::PlanningKind::Project => Self::Project,
            crate::PlanningKind::Milestone => Self::Milestone,
            crate::PlanningKind::Cycle => Self::Cycle,
            crate::PlanningKind::Target => Self::Target,
            crate::PlanningKind::Label => Self::Label,
        }
    }
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomFieldType {
    Text,
    Boolean,
    Integer,
    Date,
    Enum,
    MultiEnum,
    Identity,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomFieldDefinition {
    #[serde(rename = "type")]
    pub field_type: CustomFieldType,
    pub scopes: BTreeSet<CustomScope>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EstimateUnit {
    pub name: String,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationSchema {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub revision: Revision,
    #[serde(default)]
    pub fields: BTreeMap<String, CustomFieldDefinition>,
    #[serde(default)]
    pub unit_mode: IdentityMode,
    #[serde(default)]
    pub units: BTreeMap<String, EstimateUnit>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationSchemaRecord {
    pub definition: OrganizationSchema,
    pub path: PathBuf,
    pub source: Option<SourceToken>,
}
/// Entries replace exactly the named definitions. Absent entries remain intact;
/// archiving is explicit and identity/type repurposing is rejected.
#[derive(schemars::JsonSchema, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SchemaChange {
    pub fields: BTreeMap<String, CustomFieldDefinition>,
    pub units: BTreeMap<String, EstimateUnit>,
    pub unit_mode: Option<IdentityMode>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyViolation {
    pub path: PathBuf,
    pub field: String,
    pub message: String,
    pub historical: bool,
}
#[derive(schemars::JsonSchema, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationCompliance {
    pub compliant: bool,
    pub checked_records: usize,
    pub violations: Vec<PolicyViolation>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaChangePlan {
    pub repository: RepositoryId,
    pub source: Option<SourceToken>,
    pub definition: OrganizationSchema,
    pub fingerprint: ContentHash,
    pub allowed: bool,
    pub blockers: Vec<PmError>,
    pub compliance: OrganizationCompliance,
}
/// Explicit per-key mutation. Unknown keys and migration provenance survive
/// unless the caller deliberately names the key being replaced or removed.
#[derive(schemars::JsonSchema, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CustomPatch {
    pub set: BTreeMap<String, Value>,
    pub unset: Vec<String>,
}
impl CustomPatch {
    pub fn apply(&self, original: &BTreeMap<String, Value>) -> Result<BTreeMap<String, Value>> {
        let mut seen = BTreeSet::new();
        for key in self.set.keys().chain(self.unset.iter()) {
            text(key, "custom key")?;
            if !seen.insert(key) {
                return Err(invalid(
                    "custom patch contains a duplicate or conflicting key",
                ));
            }
        }
        let mut next = original.clone();
        next.extend(self.set.clone());
        for key in &self.unset {
            next.remove(key);
        }
        Ok(next)
    }
}

/// Exact nonnegative decimal, encoded as a canonical string. Six fractional
/// digits at most; arithmetic uses checked integer millionths, never floats.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DecimalAmount(u128);
impl DecimalAmount {
    pub fn zero() -> Self {
        Self(0)
    }
    pub fn checked_add(&self, other: &Self) -> Result<Self> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or_else(|| invalid("estimate total overflow"))
    }
}
impl FromStr for DecimalAmount {
    type Err = PmError;
    fn from_str(value: &str) -> Result<Self> {
        if value.is_empty() || value.len() > 45 {
            return Err(invalid(
                "estimate value must be a bounded nonnegative decimal string",
            ));
        }
        let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
        if whole.is_empty()
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || fraction.len() > 6
            || !fraction.bytes().all(|b| b.is_ascii_digit())
            || (value.contains('.') && fraction.is_empty())
        {
            return Err(invalid(
                "estimate value requires digits and at most six decimal places",
            ));
        }
        let whole: u128 = whole.parse().map_err(|_| invalid("estimate overflow"))?;
        let fraction: u128 = if fraction.is_empty() {
            0
        } else {
            fraction
                .parse::<u128>()
                .map_err(|_| invalid("invalid estimate"))?
                * 10_u128.pow(6 - fraction.len() as u32)
        };
        whole
            .checked_mul(1_000_000)
            .and_then(|v| v.checked_add(fraction))
            .map(Self)
            .ok_or_else(|| invalid("estimate overflow"))
    }
}
impl std::fmt::Display for DecimalAmount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let whole = self.0 / 1_000_000;
        let fraction = self.0 % 1_000_000;
        if fraction == 0 {
            write!(f, "{whole}")
        } else {
            write!(
                f,
                "{whole}.{}",
                format!("{fraction:06}").trim_end_matches('0')
            )
        }
    }
}
impl Serialize for DecimalAmount {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for DecimalAmount {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
impl schemars::JsonSchema for DecimalAmount {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DecimalAmount".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string","pattern":"^[0-9]+(\\.[0-9]{1,6})?$","description":"Exact nonnegative bounded decimal; canonical output drops trailing fractional zeroes."})
    }
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Estimate {
    pub value: DecimalAmount,
    pub unit: String,
}
impl Estimate {
    pub fn validate(&self) -> Result<()> {
        key(&self.unit, "estimate unit")
    }
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EstimateReport {
    pub by_unit: BTreeMap<String, DecimalAmount>,
    pub estimated: usize,
    pub unestimated: usize,
}

pub(crate) fn text(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 1000 || value.chars().any(char::is_control) {
        Err(invalid(format!(
            "{name} must be nonempty, single-line and at most 1000 bytes"
        )))
    } else {
        Ok(())
    }
}
pub(crate) fn key(value: &str, name: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 96
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        Err(invalid(format!(
            "{name} must be a safe 1–96 character identity"
        )))
    } else {
        Ok(())
    }
}
pub(crate) fn extensions(extra: &BTreeMap<String, Value>) -> Result<()> {
    for key in extra.keys() {
        if !key
            .strip_prefix("x-")
            .is_some_and(crate::identity::valid_slug)
        {
            return Err(invalid(format!(
                "unknown organization metadata {key:?}; use custom or x-*"
            )));
        }
    }
    Ok(())
}
pub(crate) fn check_keys<T>(values: &BTreeMap<String, T>, limit: usize, name: &str) -> Result<()> {
    if values.len() > limit {
        return Err(invalid(format!("{name} exceeds {limit} entries")));
    }
    let mut seen = BTreeSet::new();
    for id in values.keys() {
        key(id, name)?;
        if !seen.insert(id.to_ascii_lowercase()) {
            return Err(invalid(format!("case-colliding {name} identities")));
        }
    }
    Ok(())
}
impl UsersRegistry {
    pub fn validate(&self) -> Result<()> {
        check_keys(&self.users, 1024, "user")?;
        extensions(&self.extra)?;
        for user in self.users.values() {
            text(&user.name, "user name")?;
            extensions(&user.extra)?;
        }
        Ok(())
    }
}
impl OrganizationSchema {
    pub fn validate(&self) -> Result<()> {
        check_keys(&self.fields, 256, "custom field")?;
        check_keys(&self.units, 128, "estimate unit")?;
        extensions(&self.extra)?;
        for (id, field) in &self.fields {
            if id == "legacy" {
                return Err(invalid(
                    "custom.legacy is retained import provenance and cannot be declared",
                ));
            }
            if field.scopes.is_empty() {
                return Err(invalid("custom field scopes must not be empty"));
            }
            extensions(&field.extra)?;
            if matches!(
                field.field_type,
                CustomFieldType::Enum | CustomFieldType::MultiEnum
            ) {
                if field.options.is_empty() || field.options.len() > 256 {
                    return Err(invalid("enum fields need 1–256 options"));
                }
                let mut seen = BTreeSet::new();
                for option in &field.options {
                    text(option, "enum option")?;
                    if !seen.insert(option) {
                        return Err(invalid("duplicate enum option"));
                    }
                }
            } else if !field.options.is_empty() {
                return Err(invalid("only enum and multi_enum fields accept options"));
            }
        }
        for unit in self.units.values() {
            text(&unit.name, "estimate unit name")?;
            extensions(&unit.extra)?;
        }
        Ok(())
    }
}
