//! One validated retirement catalog for bulk source readers. Single-record
//! writes retain their existing read guard and historical replay admission.
use super::*;
use std::{cell::RefCell, collections::BTreeMap};

type MarkerPaths = BTreeMap<String, BTreeMap<String, Vec<PathBuf>>>;

pub(crate) struct RetirementIndex<'a, 'store> {
    receipts: BTreeMap<String, Tombstone>,
    root: &'a Path,
    snapshot: &'a Snapshot<'store>,
    config: &'a Config,
    markers: RefCell<MarkerPaths>,
}
fn key(target: &RetirementTarget) -> String {
    format!(
        "{}/{}",
        target.kind.directory(),
        target.id.to_ascii_lowercase()
    )
}
impl<'a, 'store> RetirementIndex<'a, 'store> {
    pub(crate) fn capture(
        root: &'a Path,
        snapshot: &'a Snapshot<'store>,
        config: &'a Config,
    ) -> Result<Self> {
        let receipts = retirement_receipts(snapshot, config)?
            .into_iter()
            .map(|marker| (key(&marker.target), marker))
            .collect();
        Ok(Self {
            receipts,
            root,
            snapshot,
            config,
            markers: RefCell::new(BTreeMap::new()),
        })
    }
    pub(crate) fn get(&self, target: &RetirementTarget) -> Result<Option<Tombstone>> {
        target.validate()?;
        let recorded = self.receipts.get(&key(target));
        let mut marker = None;
        let namespace = target.kind.directory();
        if !self.markers.borrow().contains_key(namespace) {
            let mut paths: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
            for path in self
                .snapshot
                .list(&Path::new("tombstones").join(namespace))?
            {
                let marker_target = target_from_path(&path)?;
                paths.entry(key(&marker_target)).or_default().push(path);
            }
            self.markers
                .borrow_mut()
                .insert(namespace.to_owned(), paths);
        }
        // Preserve the ordinary reader's selected-namespace diagnostics while
        // sharing the expensive operation parse across all requested records.
        let namespaces = self.markers.borrow();
        if let Some(paths) = namespaces
            .get(namespace)
            .and_then(|paths| paths.get(&key(target)))
        {
            for path in paths {
                if marker.is_some() {
                    return Err(corrupt("duplicate or case-colliding retirement markers")
                        .at(self.root.join(path)));
                }
                marker = Some(parse_tombstone(self.root, self.snapshot, path)?);
            }
        }
        match (marker, recorded) {
            (None, None) => Ok(None),
            (Some(marker), Some(recorded))
                if &marker == recorded && same_identity(&marker.target, target) =>
            {
                validate_retained(self.root, self.snapshot, self.config, &marker)?;
                Ok(Some(marker))
            }
            (Some(_), Some(_)) => Err(corrupt(
                "retirement marker differs from its durable receipt or exact identity",
            )
            .at(self.root.join(target.path()))),
            (Some(_), None) => Err(corrupt("retirement marker has no matching durable receipt")
                .at(self.root.join(target.path()))),
            (None, Some(_)) => Err(corrupt(
                "retired identity remains reserved but its marker has been removed",
            )
            .at(self.root.join(target.path()))),
        }
    }
}
