use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use workdeck_git::ObjectSink;

#[derive(Debug, Clone)]
pub struct ContentStore {
    root: PathBuf,
}

impl ContentStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("sha256"))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_for(&self, hash: &str) -> Result<PathBuf> {
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("invalid content hash {hash}");
        }
        Ok(self.root.join("sha256").join(&hash[..2]).join(hash))
    }

    pub fn get(&self, hash: &str) -> Result<Vec<u8>> {
        let path = self.path_for(hash)?;
        fs::read(&path).with_context(|| format!("failed to read object {}", path.display()))
    }

    pub fn contains(&self, hash: &str) -> bool {
        self.path_for(hash).is_ok_and(|path| path.is_file())
    }
}

impl ObjectSink for ContentStore {
    fn put(&mut self, bytes: &[u8]) -> Result<String> {
        let hash = format!("{:x}", Sha256::digest(bytes));
        let path = self.path_for(&hash)?;
        if path.is_file() {
            return Ok(hash);
        }
        let parent = path.parent().context("object path has no parent")?;
        fs::create_dir_all(parent)?;
        let mut temp = NamedTempFile::new_in(parent)?;
        temp.write_all(bytes)?;
        temp.as_file().sync_all()?;
        match temp.persist_noclobber(&path) {
            Ok(_) => {}
            Err(error) if path.is_file() => {
                drop(error);
            }
            Err(error) => return Err(error.error.into()),
        }
        Ok(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_objects_once() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let mut store = ContentStore::new(temporary.path()).expect("store");
        let first = store.put(b"hello").expect("put");
        let second = store.put(b"hello").expect("put again");
        assert_eq!(first, second);
        assert_eq!(store.get(&first).expect("get"), b"hello");
    }
}
