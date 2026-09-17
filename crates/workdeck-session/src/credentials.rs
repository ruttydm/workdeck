//! Owner-private Ed25519 bootstrap credentials for the local session broker.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::pkcs8::{EncodePrivateKey, EncodePublicKey};
use ed25519_dalek::{SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::{
    BrokerCommandScope, BrokerGrantBase, CallerGrant, CallerOperation, NativeSessionBrokerCrypto,
    ProducerGrant, ProducerOperation, SESSION_BROKER_SIGNATURE_ALGORITHM, SessionBrokerCrypto,
    WORKDECK_SESSION_BROKER_APP_ID, decode_base64_url, encode_base64_url,
    import_ed25519_private_key, import_ed25519_public_key,
};

const CREDENTIAL_VERSION: u32 = 1;
const CREDENTIAL_LIFETIME_MS: u64 = 10 * 365 * 24 * 60 * 60 * 1_000;
const MAX_CREDENTIAL_FILE_BYTES: u64 = 64 * 1_024;
#[cfg(unix)]
const PRIVATE_MODE: u32 = 0o600;
#[cfg(unix)]
const DIRECTORY_MODE: u32 = 0o700;

// The hard-link adoption below keeps credential publication atomic between
// processes. Within one process, serialize the complete three-file bootstrap so
// concurrent callers cannot validate the directory while another caller is
// still creating its hierarchy.
static CREDENTIAL_BOOTSTRAP_LOCK: Mutex<()> = Mutex::new(());

// A broader caller policy receives a new identity rather than silently widening
// a persisted v1 grant. Older binaries can still use their untouched caller.json.
const CALLER_CREDENTIAL_FILE: &str = "caller-v2.json";
const COMMAND_SCOPES: [&str; 9] = [
    "navigate_to_hunk",
    "reload_session",
    "comment",
    "comment_batch",
    "remove_comment",
    "clear_comments",
    "highlight",
    "clear_highlights",
    "quit_session",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum StoredRole {
    Daemon,
    Producer,
    Caller,
}

impl StoredRole {
    fn bootstrap_id(self) -> String {
        let version = if self == Self::Caller { 2 } else { 1 };
        format!("workdeck-{}-bootstrap-v{version}", self.name())
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Daemon => "daemon",
            Self::Producer => "producer",
            Self::Caller => "caller",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredCommandScope {
    name: String,
    version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredGrant {
    kind: StoredRole,
    app_id: String,
    principal_id: String,
    key_id: String,
    grant_id: String,
    algorithm: String,
    issued_at: u64,
    expires_at: u64,
    revocation_id: String,
    may_delegate: bool,
    operations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commands: Option<Vec<StoredCommandScope>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredCredentialFile {
    version: u32,
    role: StoredRole,
    key_id: String,
    public_key: String,
    private_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grant: Option<StoredGrant>,
}

#[derive(Debug, Error)]
pub enum CredentialStoreError {
    #[error(
        "Workdeck session credentials are unavailable because their owner-private runtime state is unsafe or malformed."
    )]
    Security,
    #[error("Workdeck session credential I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
pub struct SessionBrokerDaemonIdentity {
    pub key_id: String,
    pub private_key: SigningKey,
}

#[derive(Debug)]
pub struct SessionBrokerCredential<Grant> {
    pub grant: Grant,
    pub public_key: VerifyingKey,
    pub private_key: SigningKey,
}

#[derive(Debug)]
pub struct WorkdeckSessionBrokerCredentials {
    pub daemon_identity: SessionBrokerDaemonIdentity,
    pub daemon_public_key: VerifyingKey,
    pub producer: SessionBrokerCredential<ProducerGrant>,
    pub caller: SessionBrokerCredential<CallerGrant>,
}

fn expected_keys<'a>(role: StoredRole) -> &'a [&'a str] {
    match role {
        StoredRole::Daemon => &["version", "role", "keyId", "publicKey", "privateKey"],
        StoredRole::Producer | StoredRole::Caller => &[
            "version",
            "role",
            "keyId",
            "publicKey",
            "privateKey",
            "grant",
        ],
    }
}

fn has_exact_keys(record: &Map<String, Value>, keys: &[&str]) -> bool {
    record.len() == keys.len() && keys.iter().all(|key| record.contains_key(*key))
}

fn valid_key_id(value: &str) -> bool {
    value
        .strip_prefix("h_")
        .and_then(|value| value.strip_suffix("_0"))
        .is_some_and(|middle| {
            !middle.is_empty()
                && middle
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
}

fn decode_nonempty(value: &str) -> Result<Vec<u8>, CredentialStoreError> {
    decode_base64_url(value)
        .filter(|bytes| !bytes.is_empty())
        .ok_or(CredentialStoreError::Security)
}

fn expected_operations(role: StoredRole) -> &'static [&'static str] {
    match role {
        StoredRole::Daemon => &[],
        StoredRole::Producer => &["register", "reconnect"],
        StoredRole::Caller => &["list", "get", "dispatch", "diagnostics"],
    }
}

fn expected_scopes() -> Vec<StoredCommandScope> {
    COMMAND_SCOPES
        .into_iter()
        .map(|name| StoredCommandScope {
            name: name.into(),
            version: 1,
        })
        .collect()
}

fn validate_grant(
    grant: &StoredGrant,
    role: StoredRole,
    key_id: &str,
) -> Result<(), CredentialStoreError> {
    let role_name = role.name();
    let operations = expected_operations(role)
        .iter()
        .map(|operation| (*operation).to_owned())
        .collect::<Vec<_>>();
    if grant.kind != role
        || grant.app_id != WORKDECK_SESSION_BROKER_APP_ID
        || grant.principal_id != format!("workdeck-{role_name}")
        || grant.key_id != key_id
        || grant.grant_id != role.bootstrap_id()
        || grant.algorithm != SESSION_BROKER_SIGNATURE_ALGORITHM
        || grant.issued_at >= grant.expires_at
        || grant.revocation_id != role.bootstrap_id()
        || grant.may_delegate
        || grant.operations != operations
        || match role {
            StoredRole::Caller => grant.commands.as_ref() != Some(&expected_scopes()),
            StoredRole::Producer => grant.commands.is_some(),
            StoredRole::Daemon => true,
        }
    {
        return Err(CredentialStoreError::Security);
    }
    Ok(())
}

fn parse_stored(
    value: Value,
    role: StoredRole,
) -> Result<StoredCredentialFile, CredentialStoreError> {
    let record = value.as_object().ok_or(CredentialStoreError::Security)?;
    if !has_exact_keys(record, expected_keys(role)) {
        return Err(CredentialStoreError::Security);
    }
    if role != StoredRole::Daemon {
        let grant = record
            .get("grant")
            .and_then(Value::as_object)
            .ok_or(CredentialStoreError::Security)?;
        let mut grant_keys = vec![
            "kind",
            "appId",
            "principalId",
            "keyId",
            "grantId",
            "algorithm",
            "issuedAt",
            "expiresAt",
            "revocationId",
            "mayDelegate",
            "operations",
        ];
        if role == StoredRole::Caller {
            grant_keys.push("commands");
        }
        if !has_exact_keys(grant, &grant_keys) {
            return Err(CredentialStoreError::Security);
        }
    }
    let stored = serde_json::from_value::<StoredCredentialFile>(value)
        .map_err(|_| CredentialStoreError::Security)?;
    if stored.version != CREDENTIAL_VERSION || stored.role != role || !valid_key_id(&stored.key_id)
    {
        return Err(CredentialStoreError::Security);
    }
    decode_nonempty(&stored.public_key)?;
    decode_nonempty(&stored.private_key)?;
    match (role, stored.grant.as_ref()) {
        (StoredRole::Daemon, None) => {}
        (StoredRole::Producer | StoredRole::Caller, Some(grant)) => {
            validate_grant(grant, role, &stored.key_id)?;
        }
        _ => return Err(CredentialStoreError::Security),
    }
    Ok(stored)
}

#[cfg(unix)]
fn validate_owner(metadata: &fs::Metadata) -> Result<(), CredentialStoreError> {
    use std::os::unix::fs::MetadataExt;

    // SAFETY: geteuid has no preconditions and only reads the process effective user id.
    let effective_uid = unsafe { libc::geteuid() };
    (metadata.uid() == effective_uid)
        .then_some(())
        .ok_or(CredentialStoreError::Security)
}

#[cfg(not(unix))]
fn validate_owner(_: &fs::Metadata) -> Result<(), CredentialStoreError> {
    Ok(())
}

fn validate_owner_private_path(path: &Path, directory: bool) -> Result<(), CredentialStoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CredentialStoreError::Security)?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(CredentialStoreError::Security);
    }
    validate_owner(&metadata)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let unsafe_bits = if directory { 0o077 } else { 0o177 };
        if metadata.mode() & unsafe_bits != 0 {
            return Err(CredentialStoreError::Security);
        }
    }
    Ok(())
}

fn create_directory(path: &Path) -> Result<(), CredentialStoreError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(DIRECTORY_MODE);
    }
    builder.create(path)?;
    Ok(())
}

fn ensure_runtime_namespace(path: &Path) -> Result<(), CredentialStoreError> {
    create_directory(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| CredentialStoreError::Security)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CredentialStoreError::Security);
    }
    validate_owner(&metadata)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o022 != 0 {
            return Err(CredentialStoreError::Security);
        }
    }
    Ok(())
}

fn ensure_security_directory(path: &Path) -> Result<(), CredentialStoreError> {
    create_directory(path)?;
    validate_owner_private_path(path, true)
}

fn open_private_read(path: &Path) -> Result<File, CredentialStoreError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .map_err(|_| CredentialStoreError::Security)
}

fn read_private_file(path: &Path) -> Result<Value, CredentialStoreError> {
    validate_owner_private_path(path, false)?;
    let mut file = open_private_read(path)?;
    let metadata = file
        .metadata()
        .map_err(|_| CredentialStoreError::Security)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_CREDENTIAL_FILE_BYTES {
        return Err(CredentialStoreError::Security);
    }
    validate_owner(&metadata)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o177 != 0 {
            return Err(CredentialStoreError::Security);
        }
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|_| CredentialStoreError::Security)?;
    if bytes.len() as u64 != metadata.len() {
        return Err(CredentialStoreError::Security);
    }
    serde_json::from_slice(&bytes).map_err(|_| CredentialStoreError::Security)
}

fn random_bytes(length: usize) -> Result<Vec<u8>, CredentialStoreError> {
    NativeSessionBrokerCrypto
        .random_bytes(length)
        .map_err(|_| CredentialStoreError::Security)
}

fn random_id() -> Result<String, CredentialStoreError> {
    Ok(format!("h_{}_0", encode_base64_url(&random_bytes(18)?)))
}

fn temporary_suffix() -> Result<String, CredentialStoreError> {
    Ok(random_bytes(9)?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn sync_parent_directory(_path: &Path) -> Result<(), CredentialStoreError> {
    #[cfg(unix)]
    {
        let directory = File::open(_path)?;
        if let Err(error) = directory.sync_all()
            && !matches!(
                error.kind(),
                std::io::ErrorKind::InvalidInput | std::io::ErrorKind::Unsupported
            )
        {
            return Err(error.into());
        }
    }
    Ok(())
}

fn adopt_private_file(path: &Path, contents: &[u8]) -> Result<(), CredentialStoreError> {
    let temporary = path.with_extension(format!(
        "json.tmp-{}-{}",
        std::process::id(),
        temporary_suffix()?
    ));
    let result = (|| -> Result<(), CredentialStoreError> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(PRIVATE_MODE);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        match fs::hard_link(&temporary, path) {
            Ok(()) => sync_parent_directory(path.parent().ok_or(CredentialStoreError::Security)?)?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn create_stored(role: StoredRole, now: u64) -> Result<StoredCredentialFile, CredentialStoreError> {
    let seed: [u8; 32] = random_bytes(32)?
        .try_into()
        .map_err(|_| CredentialStoreError::Security)?;
    let private_key = SigningKey::from_bytes(&seed);
    let public_key = private_key.verifying_key();
    let public_der = public_key
        .to_public_key_der()
        .map_err(|_| CredentialStoreError::Security)?;
    let private_der = private_key
        .to_pkcs8_der()
        .map_err(|_| CredentialStoreError::Security)?;
    let key_id = random_id()?;
    let grant = match role {
        StoredRole::Daemon => None,
        StoredRole::Producer | StoredRole::Caller => {
            let role_name = role.name();
            Some(StoredGrant {
                kind: role,
                app_id: WORKDECK_SESSION_BROKER_APP_ID.into(),
                principal_id: format!("workdeck-{role_name}"),
                key_id: key_id.clone(),
                grant_id: role.bootstrap_id(),
                algorithm: SESSION_BROKER_SIGNATURE_ALGORITHM.into(),
                issued_at: now,
                expires_at: now
                    .checked_add(CREDENTIAL_LIFETIME_MS)
                    .ok_or(CredentialStoreError::Security)?,
                revocation_id: role.bootstrap_id(),
                may_delegate: false,
                operations: expected_operations(role)
                    .iter()
                    .map(|operation| (*operation).to_owned())
                    .collect(),
                commands: (role == StoredRole::Caller).then(expected_scopes),
            })
        }
    };
    Ok(StoredCredentialFile {
        version: CREDENTIAL_VERSION,
        role,
        key_id,
        public_key: encode_base64_url(public_der.as_bytes()),
        private_key: encode_base64_url(private_der.as_bytes()),
        grant,
    })
}

fn load_or_create(
    path: &Path,
    role: StoredRole,
    now: u64,
) -> Result<StoredCredentialFile, CredentialStoreError> {
    match read_private_file(path) {
        Ok(value) => return parse_stored(value, role),
        Err(error) => match fs::symlink_metadata(path) {
            Ok(_) => return Err(error),
            Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(error),
        },
    }
    let generated = create_stored(role, now)?;
    let mut contents =
        serde_json::to_vec(&generated).map_err(|_| CredentialStoreError::Security)?;
    contents.push(b'\n');
    adopt_private_file(path, &contents)?;
    parse_stored(read_private_file(path)?, role)
}

fn now_millis() -> Result<u64, CredentialStoreError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CredentialStoreError::Security)?
        .as_millis();
    u64::try_from(millis).map_err(|_| CredentialStoreError::Security)
}

/// Resolve the branded owner-private broker runtime directory.
#[must_use]
pub fn workdeck_session_broker_runtime_directory(env: &BTreeMap<String, String>) -> PathBuf {
    let base = env
        .get("XDG_RUNTIME_DIR")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            #[cfg(unix)]
            {
                env.get("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".workdeck"))
            }
            #[cfg(not(unix))]
            {
                None
            }
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("workdeck-mcp")
}

fn grant_base(grant: &StoredGrant) -> BrokerGrantBase {
    BrokerGrantBase {
        app_id: grant.app_id.clone(),
        principal_id: grant.principal_id.clone(),
        key_id: grant.key_id.clone(),
        grant_id: grant.grant_id.clone(),
        algorithm: grant.algorithm.clone(),
        issued_at: grant.issued_at,
        expires_at: grant.expires_at,
        revocation_id: grant.revocation_id.clone(),
        may_delegate: grant.may_delegate,
        session_id: None,
    }
}

fn producer_grant(stored: &StoredCredentialFile) -> Result<ProducerGrant, CredentialStoreError> {
    let grant = stored
        .grant
        .as_ref()
        .ok_or(CredentialStoreError::Security)?;
    Ok(ProducerGrant {
        base: grant_base(grant),
        operations: vec![ProducerOperation::Register, ProducerOperation::Reconnect],
    })
}

fn caller_grant(stored: &StoredCredentialFile) -> Result<CallerGrant, CredentialStoreError> {
    let grant = stored
        .grant
        .as_ref()
        .ok_or(CredentialStoreError::Security)?;
    Ok(CallerGrant {
        base: grant_base(grant),
        operations: vec![
            CallerOperation::List,
            CallerOperation::Get,
            CallerOperation::Dispatch,
            CallerOperation::Diagnostics,
        ],
        commands: COMMAND_SCOPES
            .into_iter()
            .map(|name| BrokerCommandScope {
                name: name.into(),
                version: 1,
            })
            .collect(),
    })
}

fn import_public(stored: &StoredCredentialFile) -> Result<VerifyingKey, CredentialStoreError> {
    import_ed25519_public_key(&decode_nonempty(&stored.public_key)?)
        .map_err(|_| CredentialStoreError::Security)
}

fn import_private(stored: &StoredCredentialFile) -> Result<SigningKey, CredentialStoreError> {
    import_ed25519_private_key(&decode_nonempty(&stored.private_key)?)
        .map_err(|_| CredentialStoreError::Security)
}

/// Load or atomically create daemon, producer, and caller bootstrap credentials.
pub fn load_or_create_workdeck_session_broker_credentials(
    env: &BTreeMap<String, String>,
    now: Option<u64>,
) -> Result<WorkdeckSessionBrokerCredentials, CredentialStoreError> {
    let _bootstrap_guard = CREDENTIAL_BOOTSTRAP_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let runtime_directory = workdeck_session_broker_runtime_directory(env);
    let security_directory = runtime_directory.join("security-v1");
    ensure_runtime_namespace(&runtime_directory)?;
    ensure_security_directory(&security_directory)?;
    let now = now.map_or_else(now_millis, Ok)?;
    let daemon = load_or_create(
        &security_directory.join("daemon.json"),
        StoredRole::Daemon,
        now,
    )?;
    let producer = load_or_create(
        &security_directory.join("producer.json"),
        StoredRole::Producer,
        now,
    )?;
    let caller = load_or_create(
        &security_directory.join(CALLER_CREDENTIAL_FILE),
        StoredRole::Caller,
        now,
    )?;
    Ok(WorkdeckSessionBrokerCredentials {
        daemon_identity: SessionBrokerDaemonIdentity {
            key_id: daemon.key_id.clone(),
            private_key: import_private(&daemon)?,
        },
        daemon_public_key: import_public(&daemon)?,
        producer: SessionBrokerCredential {
            grant: producer_grant(&producer)?,
            public_key: import_public(&producer)?,
            private_key: import_private(&producer)?,
        },
        caller: SessionBrokerCredential {
            grant: caller_grant(&caller)?,
            public_key: import_public(&caller)?,
            private_key: import_private(&caller)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    fn isolated_env(root: &TempDir) -> BTreeMap<String, String> {
        BTreeMap::from([(
            "XDG_RUNTIME_DIR".into(),
            root.path().to_string_lossy().into_owned(),
        )])
    }

    #[test]
    fn creates_stable_independent_ed25519_material_with_private_permissions() {
        let root = TempDir::new().unwrap();
        let env = isolated_env(&root);
        let first = load_or_create_workdeck_session_broker_credentials(&env, Some(100)).unwrap();
        let second = load_or_create_workdeck_session_broker_credentials(&env, Some(200)).unwrap();
        assert_eq!(second.daemon_identity.key_id, first.daemon_identity.key_id);
        assert_eq!(
            second.producer.grant.base.key_id,
            first.producer.grant.base.key_id
        );
        assert_eq!(
            second.caller.grant.base.key_id,
            first.caller.grant.base.key_id
        );
        assert_ne!(
            first.producer.grant.base.key_id,
            first.caller.grant.base.key_id
        );

        let security = root.path().join("workdeck-mcp/security-v1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                fs::symlink_metadata(&security).unwrap().mode() & 0o777,
                0o700
            );
            for name in ["daemon.json", "producer.json", CALLER_CREDENTIAL_FILE] {
                assert_eq!(
                    fs::symlink_metadata(security.join(name)).unwrap().mode() & 0o777,
                    0o600
                );
            }
        }
        let caller = fs::read_to_string(security.join(CALLER_CREDENTIAL_FILE)).unwrap();
        assert!(!caller.contains("workdeck-review-capability"));
    }

    #[test]
    fn caller_policy_upgrade_preserves_legacy_credentials_and_reuses_new_identity() {
        let root = TempDir::new().unwrap();
        let env = isolated_env(&root);
        let runtime = workdeck_session_broker_runtime_directory(&env);
        let security = runtime.join("security-v1");
        ensure_runtime_namespace(&runtime).unwrap();
        ensure_security_directory(&security).unwrap();
        for (name, role) in [
            ("daemon.json", StoredRole::Daemon),
            ("producer.json", StoredRole::Producer),
        ] {
            load_or_create(&security.join(name), role, 100).unwrap();
        }
        let mut legacy = create_stored(StoredRole::Caller, 100).unwrap();
        let grant = legacy.grant.as_mut().unwrap();
        grant.grant_id = "workdeck-caller-bootstrap-v1".into();
        grant.revocation_id = grant.grant_id.clone();
        grant
            .commands
            .as_mut()
            .unwrap()
            .retain(|scope| scope.name != "quit_session");
        adopt_private_file(
            &security.join("caller.json"),
            &serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        let preserved = ["daemon.json", "producer.json", "caller.json"]
            .map(|name| (name, fs::read(security.join(name)).unwrap()));

        let upgraded = load_or_create_workdeck_session_broker_credentials(&env, Some(200)).unwrap();
        let repeated = load_or_create_workdeck_session_broker_credentials(&env, Some(300)).unwrap();
        assert_ne!(upgraded.caller.grant.base.key_id, legacy.key_id);
        assert_eq!(
            upgraded.caller.grant.base.key_id,
            repeated.caller.grant.base.key_id
        );
        assert_eq!(
            upgraded.caller.grant.base.grant_id,
            "workdeck-caller-bootstrap-v2"
        );
        assert!(
            upgraded
                .caller
                .grant
                .commands
                .iter()
                .any(|scope| { scope.name == "quit_session" && scope.version == 1 })
        );
        for (name, bytes) in preserved {
            assert_eq!(fs::read(security.join(name)).unwrap(), bytes);
        }
        // An old grant copied into the new policy file cannot be silently widened.
        assert!(parse_stored(serde_json::to_value(legacy).unwrap(), StoredRole::Caller).is_err());
    }

    #[test]
    fn caller_policy_rejects_added_missing_or_version_changed_commands() {
        let stored = create_stored(StoredRole::Caller, 100).unwrap();
        let canonical = serde_json::to_value(stored).unwrap();
        for change in 0..3 {
            let mut value = canonical.clone();
            let commands = value["grant"]["commands"].as_array_mut().unwrap();
            match change {
                0 => commands.push(json!({"name": "shutdown_daemon", "version": 1})),
                1 => {
                    commands.pop();
                }
                _ => commands[0]["version"] = json!(2),
            }
            assert!(parse_stored(value, StoredRole::Caller).is_err());
        }
    }

    #[test]
    fn adopts_one_complete_winner_under_concurrent_first_use() {
        let root = TempDir::new().unwrap();
        let env = Arc::new(isolated_env(&root));
        let barrier = Arc::new(Barrier::new(12));
        let handles = (0..12)
            .map(|_| {
                let env = Arc::clone(&env);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    load_or_create_workdeck_session_broker_credentials(&env, Some(100)).unwrap()
                })
            })
            .collect::<Vec<_>>();
        let credentials = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert!(credentials.windows(2).all(|pair| {
            pair[0].daemon_identity.key_id == pair[1].daemon_identity.key_id
                && pair[0].producer.grant.base.key_id == pair[1].producer.grant.base.key_id
                && pair[0].caller.grant.base.key_id == pair[1].caller.grant.base.key_id
        }));
    }

    #[test]
    fn rejects_malformed_and_overly_permissive_files_without_leaking_private_bytes() {
        let root = TempDir::new().unwrap();
        let env = isolated_env(&root);
        load_or_create_workdeck_session_broker_credentials(&env, Some(100)).unwrap();
        let caller = root
            .path()
            .join("workdeck-mcp/security-v1")
            .join(CALLER_CREDENTIAL_FILE);
        let secret = "private-secret-sentinel";
        fs::write(&caller, format!(r#"{{"privateKey":"{secret}"}}"#)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&caller, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let message = load_or_create_workdeck_session_broker_credentials(&env, Some(100))
            .unwrap_err()
            .to_string();
        assert!(message.contains("unsafe or malformed"));
        assert!(!message.contains(secret));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlinked_security_directory() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().unwrap();
        let env = isolated_env(&root);
        let runtime = root.path().join("workdeck-mcp");
        let redirect = root.path().join("redirect");
        create_directory(&runtime).unwrap();
        create_directory(&redirect).unwrap();
        symlink(&redirect, runtime.join("security-v1")).unwrap();
        assert!(
            load_or_create_workdeck_session_broker_credentials(&env, Some(100))
                .unwrap_err()
                .to_string()
                .contains("unsafe or malformed")
        );
    }

    #[test]
    fn rejects_noncanonical_keys_and_grants_with_broadened_scopes() {
        let root = TempDir::new().unwrap();
        let env = isolated_env(&root);
        load_or_create_workdeck_session_broker_credentials(&env, Some(100)).unwrap();
        let caller = root
            .path()
            .join("workdeck-mcp/security-v1")
            .join(CALLER_CREDENTIAL_FILE);
        let mut value: Value = serde_json::from_slice(&fs::read(&caller).unwrap()).unwrap();
        value["grant"]["operations"] =
            json!(["list", "get", "dispatch", "diagnostics", "shutdown"]);
        let mut contents = serde_json::to_vec(&value).unwrap();
        contents.push(b'\n');
        fs::write(&caller, contents).unwrap();
        assert!(load_or_create_workdeck_session_broker_credentials(&env, Some(100)).is_err());

        value["grant"]["operations"] = json!(["list", "get", "dispatch", "diagnostics"]);
        value["publicKey"] = json!("AA==");
        fs::write(&caller, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(load_or_create_workdeck_session_broker_credentials(&env, Some(100)).is_err());
    }
}
