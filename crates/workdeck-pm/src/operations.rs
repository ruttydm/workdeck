use crate::{
    Repository, Result,
    transactions::{MutationReceipt, PendingOperation},
};

impl Repository {
    /// Source-qualified durable planning mutations. Imported annotations are
    /// a separate stream; this history does not qualify check/CI outcomes.
    pub fn operation_history(&self) -> Result<Vec<MutationReceipt>> {
        use crate::{ErrorCode, PmError};
        use std::{collections::BTreeSet, path::Path};
        self.store()?.with_snapshot(|snapshot| {
            crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let paths = snapshot.list_bounded(Path::new("operations"), 10_000)?;
            let mut total = 0usize;
            let mut requests = BTreeSet::new();
            let mut receipts = Vec::new();
            for path in paths {
                let bytes = snapshot
                    .read_bounded(&path, 64 * 1024 * 1024 - total)?
                    .ok_or_else(|| {
                        PmError::new(ErrorCode::CorruptStore, "operation receipt disappeared")
                            .at(&path)
                    })?;
                total += bytes.len();
                let receipt: MutationReceipt =
                    serde_yaml_ng::from_slice(&bytes).map_err(|error| {
                        PmError::new(ErrorCode::CorruptStore, error.to_string()).at(&path)
                    })?;
                crate::transactions::validate_receipt(&receipt)?;
                crate::graph::validate_receipt(&receipt)?;
                crate::wiki::validate_receipt(&receipt)?;
                crate::saved_views::validate_receipt(&receipt)?;
                crate::features::validate_receipt(&receipt)?;
                crate::gates::store::validate_receipt(&receipt)?;
                crate::evidence::store::validate_receipt(&receipt)?;
                crate::attestations::records::validate_receipt(&receipt)?;
                crate::retained_reviews::records::validate_receipt(&receipt)?;
                crate::questions::validate_receipt(&receipt)?;
                crate::handoffs::validate_receipt(&receipt)?;
                crate::execution::records::validate_receipt(&receipt)?;
                crate::claims::validate_receipt(&receipt)?;
                crate::completion::validate_receipt(&receipt)?;
                if receipt.repository.as_ref() != Some(self.identity())
                    || path != Path::new(&format!("operations/{}.yml", receipt.operation_id))
                    || !requests.insert(receipt.request_id.clone())
                {
                    return Err(PmError::new(
                        ErrorCode::CorruptStore,
                        "operation source, path or unique request identity is invalid",
                    )
                    .at(path));
                }
                receipts.push(receipt);
            }
            receipts.sort_by(|a, b| b.operation_id.cmp(&a.operation_id));
            Ok(receipts)
        })
    }

    /// Inspection deliberately works through a source opened for recovery.
    /// It never advances a journal or modifies authoritative planning files.
    pub fn pending_operations(&self) -> Result<Vec<PendingOperation>> {
        self.store()?.pending_operations()
    }

    /// Finish previously published durable intent. Every before/after content
    /// precondition is rechecked; conflicting direct edits remain untouched.
    pub fn recover_operations(&self) -> Result<Vec<MutationReceipt>> {
        self.store()?.recover()
    }
}
