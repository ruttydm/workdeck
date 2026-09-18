//! Document callback bookkeeping for multiplexed native extension input loops.

use crate::{
    EXTENSION_DOCUMENT_READ_METHOD, ExtensionDocumentReadRequest, ExtensionFileSide, JsonRpcError,
    JsonRpcRequest,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{self, Write};

/// A completed callback, attributed to its original host request and source side.
#[derive(Debug)]
pub struct ExtensionDocumentReply {
    pub parent_id: u64,
    pub side: ExtensionFileSide,
    pub result: Result<Option<String>, JsonRpcError>,
}

/// Nonblocking callback state for an extension-owned input loop.
///
/// The caller keeps reading and dispatching host requests while callbacks are
/// outstanding. Child IDs are never reused, including after cancellation or a
/// failed write. This object does not own transport framing or I/O deadlines.
#[derive(Default)]
pub struct ExtensionDocumentCallbacks {
    next_id: u64,
    pending: BTreeMap<u64, (u64, ExtensionFileSide)>,
}

impl ExtensionDocumentCallbacks {
    /// Send a callback without waiting for its response. At most 32 callbacks
    /// per parent and 128 across all parents can be outstanding.
    pub fn request(
        &mut self,
        output: &mut impl Write,
        parent_id: u64,
        side: ExtensionFileSide,
    ) -> io::Result<u64> {
        if self.pending.len() >= 128
            || self
                .pending
                .values()
                .filter(|(id, _)| *id == parent_id)
                .count()
                >= 32
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "document callback limit",
            ));
        }
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("document callback ID space exhausted"))?;
        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id,
            method: EXTENSION_DOCUMENT_READ_METHOD.into(),
            params: serde_json::to_value(ExtensionDocumentReadRequest {
                parent_request_id: parent_id,
                side,
            })
            .map_err(io::Error::other)?,
        };
        serde_json::to_writer(&mut *output, &request).map_err(io::Error::other)?;
        output.write_all(b"\n")?;
        output.flush()?;
        self.pending.insert(id, (parent_id, side));
        Ok(id)
    }

    /// Route a response; requests, notifications, and unknown/stale IDs remain
    /// unconsumed. Malformed known responses are errors, never unreadable text.
    pub fn accept(&mut self, frame: &Value) -> io::Result<Option<ExtensionDocumentReply>> {
        if frame.get("method").is_some() {
            return Ok(None);
        }
        let Some(id) = frame.get("id").and_then(Value::as_u64) else {
            return Ok(None);
        };
        let Some(&(parent_id, side)) = self.pending.get(&id) else {
            return Ok(None);
        };
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid document response");
        if frame.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(invalid());
        }
        let result = match (frame.get("result"), frame.get("error")) {
            (Some(Value::Null), None) => Ok(None),
            (Some(Value::String(text)), None) => Ok(Some(text.clone())),
            (None, Some(error)) => {
                Err(serde_json::from_value(error.clone()).map_err(|_| invalid())?)
            }
            _ => return Err(invalid()),
        };
        self.pending.remove(&id);
        Ok(Some(ExtensionDocumentReply {
            parent_id,
            side,
            result,
        }))
    }

    /// Retire only this parent's callbacks. Later responses cannot be delivered
    /// to a replacement request, since child IDs are monotonically allocated.
    pub fn retire(&mut self, parent_id: u64) {
        self.pending.retain(|_, (id, _)| *id != parent_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reversed_callbacks_keep_parent_and_side_identity() {
        let mut callbacks = ExtensionDocumentCallbacks::default();
        let mut output = Vec::new();
        let old = callbacks
            .request(&mut output, 9, ExtensionFileSide::Old)
            .unwrap();
        let new = callbacks
            .request(&mut output, 8, ExtensionFileSide::New)
            .unwrap();
        for (id, parent, side, text) in [
            (new, 8, ExtensionFileSide::New, Some("λ\n")),
            (old, 9, ExtensionFileSide::Old, None),
        ] {
            let reply = callbacks
                .accept(&serde_json::json!({"jsonrpc":"2.0","id":id,"result":text}))
                .unwrap()
                .unwrap();
            assert_eq!(reply.parent_id, parent);
            assert_eq!(reply.side, side);
            assert_eq!(reply.result.unwrap().as_deref(), text);
        }
        assert!(callbacks.pending.is_empty());
        let requests = String::from_utf8(output).unwrap();
        let requests = requests
            .lines()
            .map(|line| serde_json::from_str::<JsonRpcRequest>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            requests[0].params,
            serde_json::json!({"parentRequestId":9,"side":"old"})
        );
        assert_eq!(
            requests[1].params,
            serde_json::json!({"parentRequestId":8,"side":"new"})
        );
    }

    #[test]
    fn retirement_preserves_peers_and_does_not_reuse_ids() {
        let mut callbacks = ExtensionDocumentCallbacks::default();
        let first = callbacks
            .request(&mut Vec::new(), 1, ExtensionFileSide::New)
            .unwrap();
        let peer = callbacks
            .request(&mut Vec::new(), 2, ExtensionFileSide::New)
            .unwrap();
        callbacks.retire(1);
        callbacks.retire(1);
        let replacement = callbacks
            .request(&mut Vec::new(), 1, ExtensionFileSide::New)
            .unwrap();
        assert!(replacement > peer && peer > first);
        assert!(
            callbacks
                .accept(&serde_json::json!({"jsonrpc":"2.0","id":first,"result":"stale"}))
                .unwrap()
                .is_none()
        );
        assert_eq!(callbacks.pending.len(), 2);
        assert!(
            callbacks
                .accept(&serde_json::json!({"jsonrpc":"2.0","id":peer,"method":"host/request"}))
                .unwrap()
                .is_none()
        );
        let reply = callbacks.accept(&serde_json::json!({"jsonrpc":"2.0","id":peer,"error":{"code":-32602,"message":"rejected"}})).unwrap().unwrap();
        assert_eq!(reply.parent_id, 2);
        assert_eq!(reply.result.unwrap_err().message, "rejected");
    }

    #[test]
    fn invalid_response_does_not_consume_callback() {
        let mut callbacks = ExtensionDocumentCallbacks::default();
        let id = callbacks
            .request(&mut Vec::new(), 1, ExtensionFileSide::New)
            .unwrap();
        for extra in [
            serde_json::json!({"result":null,"error":null}),
            serde_json::json!({"result":7}),
            serde_json::json!({"error":null}),
            serde_json::json!({"error":{"code":"bad","message":"bad"}}),
        ] {
            let mut frame = extra;
            frame["jsonrpc"] = serde_json::json!("2.0");
            frame["id"] = serde_json::json!(id);
            assert_eq!(
                callbacks.accept(&frame).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(callbacks.pending.len(), 1);
        }
    }

    #[test]
    fn limits_and_exhaustion_do_not_write_or_displace_pending_requests() {
        let mut callbacks = ExtensionDocumentCallbacks::default();
        for parent in 0..4 {
            for _ in 0..32 {
                callbacks
                    .request(&mut Vec::new(), parent, ExtensionFileSide::New)
                    .unwrap();
            }
            let mut output = Vec::new();
            assert_eq!(
                callbacks
                    .request(&mut output, parent, ExtensionFileSide::New)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::WouldBlock
            );
            assert!(output.is_empty());
        }
        assert_eq!(
            callbacks
                .request(&mut Vec::new(), 4, ExtensionFileSide::New)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        callbacks.retire(0);
        callbacks.next_id = u64::MAX;
        let mut output = Vec::new();
        assert!(
            callbacks
                .request(&mut output, 4, ExtensionFileSide::New)
                .is_err()
        );
        assert!(output.is_empty());
        assert_eq!(callbacks.pending.len(), 96);
    }
}
