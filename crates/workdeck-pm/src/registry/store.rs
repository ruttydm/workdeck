use super::{
    types::{Document, Receipt},
    *,
};
use crate::{RequestId, Revision, SourceToken, execution::local, sources::fs};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions, TryLockError},
    path::PathBuf,
    time::{Duration, Instant},
};

/// The owner pins the local registry location. Registered sources remain independent.
#[derive(Debug, Clone)]
pub struct RegistryStore {
    owner: Repository,
    identity: fs::Identity,
    directory: PathBuf,
}

impl RegistryStore {
    /// Read-only construction. Empty registries do not create local files.
    pub fn open(owner: &Repository) -> Result<Self> {
        let root = owner
            .root()
            .canonicalize()
            .map_err(|e| PmError::io(owner.root(), e))?;
        // Check the caller's selected source before accepting canonical spelling.
        local::safe(owner.root(), owner.root())?;
        let identity = fs::directory(&root)?;
        let selected = Repository::open_source(&root)?;
        if selected.identity() != owner.identity() {
            return Err(stale("registry owner changed"));
        }
        Ok(Self {
            directory: root.join(".local/repositories"),
            owner: selected,
            identity,
        })
    }

    fn verify_owner(&self) -> Result<()> {
        if fs::directory(self.owner.root())? != self.identity
            || Repository::open_source(self.owner.root())?.identity() != self.owner.identity()
        {
            return Err(stale("registry owner directory or repository changed"));
        }
        Ok(())
    }

    fn read(&self) -> Result<(Document, Option<Vec<u8>>)> {
        self.verify_owner()?;
        let path = self.directory.join("registry.json");
        local::safe(self.owner.root(), &path)?;
        let bytes = match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(PmError::io(&path, e)),
            Ok(_) => Some(fs::read(&path, MAX_REGISTRY_BYTES)?.0),
        };
        let doc = match &bytes {
            Some(bytes) => serde_json::from_slice::<Document>(bytes)
                .map_err(|e| invalid(format!("invalid local registry: {e}")).at(&path))?,
            None => Document {
                schema: 1,
                owner: self.owner.identity().clone(),
                revision: Revision::INITIAL,
                entries: BTreeMap::new(),
                requests: BTreeMap::new(),
            },
        };
        if doc.schema != 1 {
            return Err(
                PmError::new(ErrorCode::Unsupported, "unsupported local registry schema").at(&path),
            );
        }
        if doc.owner != *self.owner.identity() {
            return Err(stale("local registry belongs to another repository"));
        }
        if doc.entries.len() > MAX_REGISTERED_CHECKOUTS
            || doc.requests.len() > MAX_REGISTRY_REQUESTS
        {
            return Err(invalid("local registry exceeds its entry or request bound"));
        }
        for (alias, entry) in &doc.entries {
            validate_alias(alias)?;
            if alias != &entry.alias || !entry.checkout.is_absolute() {
                return Err(invalid("invalid local checkout mapping"));
            }
        }
        for (request, receipt) in &doc.requests {
            if request != receipt.outcome.request.as_str()
                || receipt.outcome.replayed
                || receipt.outcome.revision > doc.revision
            {
                return Err(invalid("invalid local registry request history"));
            }
        }
        self.verify_owner()?;
        Ok((doc, bytes))
    }

    pub fn snapshot(&self) -> Result<RegistrySnapshot> {
        let (doc, bytes) = self.read()?;
        Ok(RegistrySnapshot {
            owner: doc.owner,
            source: SourceToken::new(doc.revision, bytes.as_deref().unwrap_or_default()),
            entries: doc.entries.into_values().collect(),
        })
    }

    pub fn resolve(&self, alias: &str) -> Result<(RegisteredCheckout, Repository)> {
        validate_alias(alias)?;
        let (doc, _) = self.read()?;
        let entry = doc
            .entries
            .get(alias)
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "checkout alias is not registered"))?;
        Ok((entry.clone(), entry.resolve()?))
    }

    pub fn mutate(&self, input: &RegistryRequest, request: &RequestId) -> Result<RegistryOutcome> {
        self.mutate_with_faults(input, request, |_| Ok(()))
    }

    #[doc(hidden)]
    pub fn mutate_with_faults(
        &self,
        input: &RegistryRequest,
        request: &RequestId,
        mut fault: impl FnMut(RegistryFaultPoint) -> Result<()>,
    ) -> Result<RegistryOutcome> {
        let input_hash =
            ContentHash::of(&serde_json::to_vec(input).map_err(|e| invalid(e.to_string()))?);
        self.verify_owner()?;
        local::directory(self.owner.root(), &self.directory)?;
        let directory_identity = fs::directory(&self.directory)?;
        let lock_path = self.directory.join("writer.lock");
        local::safe(self.owner.root(), &lock_path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let lock = options
            .open(&lock_path)
            .map_err(|e| PmError::io(&lock_path, e))?;
        if !lock
            .metadata()
            .map_err(|e| PmError::io(&lock_path, e))?
            .is_file()
        {
            return Err(invalid("registry lock must be a regular file"));
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(PmError::new(
                        ErrorCode::Locked,
                        "another writer owns this local registry",
                    ));
                }
                Err(TryLockError::Error(e)) => return Err(PmError::io(&lock_path, e)),
            }
        }
        let lock_identity = file_identity(&lock)?;
        if fs::read(&lock_path, 0)?.1 != lock_identity {
            return Err(stale("registry lock changed while waiting"));
        }
        let (mut doc, before) = self.read()?;
        if let Some(receipt) = doc.requests.get(request.as_str()) {
            if receipt.input != input_hash {
                return Err(PmError::new(
                    ErrorCode::Conflict,
                    "registry request ID was used for different input",
                ));
            }
            let mut outcome = receipt.outcome.clone();
            outcome.replayed = true;
            return Ok(outcome);
        }
        if SourceToken::new(doc.revision, before.as_deref().unwrap_or_default()) != input.expected {
            return Err(stale("registry changed after inspection"));
        }
        if doc.requests.len() >= MAX_REGISTRY_REQUESTS {
            return Err(invalid(
                "local registry request history is full; preserve it before creating a new registry",
            ));
        }
        let checkout = match &input.mutation {
            RegistryMutation::Register { checkout } => {
                checkout.resolve()?;
                if doc.entries.contains_key(&checkout.alias) {
                    return Err(PmError::new(
                        ErrorCode::Conflict,
                        "checkout alias already exists; remove it explicitly before replacing the mapping",
                    ));
                }
                if doc.entries.len() >= MAX_REGISTERED_CHECKOUTS {
                    return Err(invalid("local registry contains 256 checkouts"));
                }
                doc.entries.insert(checkout.alias.clone(), checkout.clone());
                checkout.clone()
            }
            RegistryMutation::Remove { alias } => {
                validate_alias(alias)?;
                doc.entries.remove(alias).ok_or_else(|| {
                    PmError::new(ErrorCode::NotFound, "checkout alias is not registered")
                })?
            }
        };
        doc.revision = doc.revision.next()?;
        let outcome = RegistryOutcome {
            request: request.clone(),
            revision: doc.revision,
            checkout,
            replayed: false,
        };
        doc.requests.insert(
            request.as_str().to_owned(),
            Receipt {
                input: input_hash,
                outcome: outcome.clone(),
            },
        );
        let encoded = serde_json::to_vec_pretty(&doc).map_err(|e| invalid(e.to_string()))?;
        if encoded.len() > MAX_REGISTRY_BYTES {
            return Err(invalid("local registry exceeds 1 MiB"));
        }
        fault(RegistryFaultPoint::BeforePublish)?;
        if let RegistryMutation::Register { checkout } = &input.mutation {
            checkout.resolve()?;
        }
        self.verify_owner()?;
        if fs::directory(&self.directory)? != directory_identity
            || fs::read(&lock_path, 0)?.1 != lock_identity
        {
            return Err(stale("registry directory or lock changed"));
        }
        let ignore = self.directory.join(".gitignore");
        match local::read(self.owner.root(), &ignore, 4096)? {
            Some(bytes) if bytes == b"*\n" => (),
            Some(_) => {
                return Err(PmError::new(
                    ErrorCode::Conflict,
                    "preserve existing local registry ignore policy",
                ));
            }
            None => local::publish(self.owner.root(), &ignore, b"*\n", None)?,
        }
        local::publish(
            self.owner.root(),
            &self.directory.join("registry.json"),
            &encoded,
            before.as_deref().map(ContentHash::of).as_ref(),
        )?;
        fault(RegistryFaultPoint::AfterPublish)?;
        // Keep the owned lock alive through durable publication.
        drop(lock);
        Ok(outcome)
    }
}

#[cfg(unix)]
fn file_identity(file: &File) -> Result<fs::Identity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().map_err(|e| invalid(e.to_string()))?;
    Ok(fs::Identity(metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn file_identity(_: &File) -> Result<fs::Identity> {
    Err(PmError::new(
        ErrorCode::Unsupported,
        "registry locking requires qualified descriptor identities",
    ))
}

fn stale(message: &str) -> PmError {
    PmError::new(ErrorCode::StaleSource, message)
}
