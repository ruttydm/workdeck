//! Hard byte ceilings for HTTP bodies, responses, WebSocket messages, and retained review data.

use crate::{
    BrokerCapacityError, BudgetError, DEFAULT_SESSION_BROKER_LIMITS, ReservationGroup,
    ResourceBudget,
};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;

pub const MAX_HTTP_BODY_BYTES: u64 = DEFAULT_SESSION_BROKER_LIMITS.max_http_body_bytes;
pub const MAX_WS_MESSAGE_BYTES: u64 = DEFAULT_SESSION_BROKER_LIMITS.max_ws_message_bytes;
pub const MAX_REGISTRATION_FILES: usize = 5_000;
pub const MAX_REGISTRATION_HUNKS_PER_FILE: usize = 10_000;
pub const MAX_REGISTRATION_PATCH_BYTES: usize = 2 * 1_024 * 1_024;
pub const MAX_SNAPSHOT_LIVE_COMMENTS: usize = 10_000;
pub const MAX_SNAPSHOT_REVIEW_NOTES: usize = 10_000;

#[derive(Debug)]
pub enum BrokerBodyLimitError {
    PayloadTooLarge { limit_bytes: u64 },
    InvalidContentLength,
    Capacity(BrokerCapacityError),
    InactiveReservation(String),
    Io(std::io::Error),
    InvalidUtf8(std::str::Utf8Error),
}

impl fmt::Display for BrokerBodyLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayloadTooLarge { limit_bytes } => {
                write!(
                    formatter,
                    "Payload exceeds the {limit_bytes}-byte session broker limit."
                )
            }
            Self::InvalidContentLength => {
                formatter.write_str("Content-Length must be a canonical non-negative integer.")
            }
            Self::Capacity(error) => error.fmt(formatter),
            Self::InactiveReservation(message) => formatter.write_str(message),
            Self::Io(error) => error.fmt(formatter),
            Self::InvalidUtf8(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for BrokerBodyLimitError {}

impl From<BrokerCapacityError> for BrokerBodyLimitError {
    fn from(error: BrokerCapacityError) -> Self {
        Self::Capacity(error)
    }
}

impl From<BudgetError> for BrokerBodyLimitError {
    fn from(error: BudgetError) -> Self {
        match error {
            BudgetError::Capacity(error) => Self::Capacity(error),
            BudgetError::Inactive(message) => Self::InactiveReservation(message),
            BudgetError::ReleasedGroup => {
                Self::InactiveReservation("reservation group was already released".into())
            }
        }
    }
}

impl From<std::io::Error> for BrokerBodyLimitError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub trait BrokerBody: Read + Send {
    fn cancel(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl BrokerBody for std::io::Cursor<Vec<u8>> {}

/// Count the bytes a JavaScript string's UTF-16 code units produce through `TextEncoder`.
#[must_use]
pub fn utf8_byte_length_utf16(value: &[u16]) -> usize {
    let mut bytes = 0;
    let mut index = 0;
    while index < value.len() {
        let code_unit = value[index];
        if code_unit <= 0x7f {
            bytes += 1;
        } else if code_unit <= 0x7ff {
            bytes += 2;
        } else if (0xd800..=0xdbff).contains(&code_unit)
            && value
                .get(index + 1)
                .is_some_and(|next| (0xdc00..=0xdfff).contains(next))
        {
            bytes += 4;
            index += 1;
        } else {
            bytes += 3;
        }
        index += 1;
    }
    bytes
}

#[must_use]
pub const fn utf8_byte_length(value: &str) -> usize {
    value.len()
}

fn parse_content_length(value: Option<&str>) -> Result<Option<u128>, BrokerBodyLimitError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value == "0" {
        return Ok(Some(0));
    }
    if value.is_empty()
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(BrokerBodyLimitError::InvalidContentLength);
    }
    Ok(Some(value.parse().unwrap_or(u128::MAX)))
}

#[derive(Debug)]
pub struct ReservedBodyBytes {
    pub bytes: Vec<u8>,
    pub reservation: ReservationGroup,
}

pub fn read_request_bytes_with_reservation(
    body: Option<&mut dyn BrokerBody>,
    declared_content_length: Option<&str>,
    max_bytes: u64,
    aggregate_budget: Option<&ResourceBudget>,
) -> Result<ReservedBodyBytes, BrokerBodyLimitError> {
    let mut body = body;
    let declared = match parse_content_length(declared_content_length) {
        Ok(value) => value,
        Err(error) => {
            if let Some(body) = body.as_deref_mut() {
                let _ = body.cancel();
            }
            return Err(error);
        }
    };
    if declared.is_some_and(|declared| {
        declared > u128::from(max_bytes) || declared > 9_007_199_254_740_991
    }) {
        if let Some(body) = body.as_deref_mut() {
            let _ = body.cancel();
        }
        return Err(BrokerBodyLimitError::PayloadTooLarge {
            limit_bytes: max_bytes,
        });
    }
    let body = match body {
        Some(body) => body,
        None => {
            return Ok(ReservedBodyBytes {
                bytes: Vec::new(),
                reservation: ReservationGroup::default(),
            });
        }
    };

    let mut source_reservation = if let Some(budget) = aggregate_budget {
        match budget.reserve(max_bytes) {
            Ok(reservation) => Some(reservation),
            Err(error) => {
                let _ = body.cancel();
                return Err(error.into());
            }
        }
    } else {
        None
    };
    let result = (|| {
        let capacity = usize::try_from(max_bytes.min(64 * 1_024)).unwrap_or(64 * 1_024);
        let mut bytes = Vec::new();
        let mut chunk = vec![0_u8; capacity.max(1)];
        loop {
            let read = body.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            let next = bytes.len().saturating_add(read);
            if u64::try_from(next).unwrap_or(u64::MAX) > max_bytes {
                let _ = body.cancel();
                return Err(BrokerBodyLimitError::PayloadTooLarge {
                    limit_bytes: max_bytes,
                });
            }
            bytes.extend_from_slice(&chunk[..read]);
        }

        let mut retained = ReservationGroup::default();
        if let (Some(budget), Some(source)) = (aggregate_budget, source_reservation.take()) {
            let exact = match budget.resize(&source, bytes.len() as u64) {
                Ok(exact) => exact,
                Err(error) => {
                    source.release();
                    return Err(error.into());
                }
            };
            let retained_copy = match budget.reserve(bytes.len() as u64) {
                Ok(reservation) => reservation,
                Err(error) => {
                    exact.release();
                    return Err(error.into());
                }
            };
            retained.add(retained_copy)?;
            exact.release();
        }
        Ok(ReservedBodyBytes {
            bytes,
            reservation: retained,
        })
    })();
    if result.is_err() {
        if let Some(reservation) = source_reservation.take() {
            reservation.release();
        }
        let _ = body.cancel();
    }
    result
}

pub fn read_request_bytes_with_limit(
    body: Option<&mut dyn BrokerBody>,
    declared_content_length: Option<&str>,
    max_bytes: u64,
) -> Result<Vec<u8>, BrokerBodyLimitError> {
    let mut read =
        read_request_bytes_with_reservation(body, declared_content_length, max_bytes, None)?;
    read.reservation.release();
    Ok(read.bytes)
}

pub fn read_request_text_with_limit(
    body: Option<&mut dyn BrokerBody>,
    declared_content_length: Option<&str>,
    max_bytes: u64,
) -> Result<String, BrokerBodyLimitError> {
    let bytes = read_request_bytes_with_limit(body, declared_content_length, max_bytes)?;
    let text = std::str::from_utf8(&bytes).map_err(BrokerBodyLimitError::InvalidUtf8)?;
    Ok(text.into())
}

pub struct BrokerHttpResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<BoundedHttpBody>,
}

pub enum BoundedHttpBody {
    Streaming(Box<dyn BrokerBody>),
    Retained(RetainedResponseBody),
}

impl BoundedHttpBody {
    pub fn read_all(mut self) -> Result<Vec<u8>, std::io::Error> {
        match &mut self {
            Self::Streaming(body) => {
                let mut bytes = Vec::new();
                body.read_to_end(&mut bytes)?;
                Ok(bytes)
            }
            Self::Retained(body) => Ok(body.take()),
        }
    }

    pub fn cancel(&mut self) -> std::io::Result<()> {
        match self {
            Self::Streaming(body) => body.cancel(),
            Self::Retained(body) => {
                body.cancel();
                Ok(())
            }
        }
    }
}

pub struct RetainedResponseBody {
    bytes: Option<Vec<u8>>,
    reservation: ReservationGroup,
}

impl RetainedResponseBody {
    pub fn take(&mut self) -> Vec<u8> {
        let bytes = self.bytes.take().unwrap_or_default();
        self.reservation.release();
        bytes
    }

    pub fn cancel(&mut self) {
        self.bytes = None;
        self.reservation.release();
    }
}

fn header<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn service_unavailable() -> BrokerHttpResponse {
    BrokerHttpResponse {
        status: 503,
        status_text: String::new(),
        headers: BTreeMap::new(),
        body: None,
    }
}

pub fn bound_http_response(
    mut response: BrokerHttpResponse,
    max_bytes: u64,
    aggregate_budget: Option<&ResourceBudget>,
) -> Result<BrokerHttpResponse, BrokerBodyLimitError> {
    if header(&response.headers, "content-type")
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("text/event-stream"))
    {
        return Ok(response);
    }
    let declared_oversized = header(&response.headers, "content-length")
        .and_then(|value| parse_content_length(Some(value)).ok().flatten())
        .is_some_and(|length| length > u128::from(max_bytes));
    if declared_oversized {
        if let Some(body) = &mut response.body {
            let _ = body.cancel();
        }
        return Ok(service_unavailable());
    }
    let Some(body) = response.body.take() else {
        return Ok(response);
    };
    let mut streaming = match body {
        BoundedHttpBody::Streaming(body) => body,
        BoundedHttpBody::Retained(mut retained) => {
            Box::new(std::io::Cursor::new(retained.take())) as Box<dyn BrokerBody>
        }
    };
    let read = read_request_bytes_with_reservation(
        Some(streaming.as_mut()),
        None,
        max_bytes,
        aggregate_budget,
    );
    let read = match read {
        Ok(read) => read,
        Err(BrokerBodyLimitError::PayloadTooLarge { .. } | BrokerBodyLimitError::Capacity(_)) => {
            let _ = streaming.cancel();
            return Ok(service_unavailable());
        }
        Err(error) => return Err(error),
    };
    response
        .headers
        .retain(|key, _| !key.eq_ignore_ascii_case("content-length"));
    response
        .headers
        .insert("content-length".into(), read.bytes.len().to_string());
    response.body = Some(BoundedHttpBody::Retained(RetainedResponseBody {
        bytes: Some(read.bytes),
        reservation: read.reservation,
    }));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct TrackingBody {
        cursor: Cursor<Vec<u8>>,
        pulls: Arc<AtomicUsize>,
        cancelled: Arc<AtomicBool>,
        chunk_size: usize,
    }

    impl TrackingBody {
        fn new(bytes: Vec<u8>, chunk_size: usize) -> (Self, Arc<AtomicUsize>, Arc<AtomicBool>) {
            let pulls = Arc::new(AtomicUsize::new(0));
            let cancelled = Arc::new(AtomicBool::new(false));
            (
                Self {
                    cursor: Cursor::new(bytes),
                    pulls: Arc::clone(&pulls),
                    cancelled: Arc::clone(&cancelled),
                    chunk_size,
                },
                pulls,
                cancelled,
            )
        }
    }

    impl Read for TrackingBody {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.pulls.fetch_add(1, Ordering::Relaxed);
            let length = buffer.len().min(self.chunk_size);
            self.cursor.read(&mut buffer[..length])
        }
    }

    impl BrokerBody for TrackingBody {
        fn cancel(&mut self) -> std::io::Result<()> {
            self.cancelled.store(true, Ordering::Relaxed);
            Ok(())
        }
    }

    fn streaming_response(body: TrackingBody, length: Option<&str>) -> BrokerHttpResponse {
        let mut headers = BTreeMap::new();
        if let Some(length) = length {
            headers.insert("content-length".into(), length.into());
        }
        BrokerHttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers,
            body: Some(BoundedHttpBody::Streaming(Box::new(body))),
        }
    }

    #[test]
    fn cancels_oversized_or_malformed_declared_bodies_before_pulling() {
        for (declared, expected_invalid) in [("10485760", false), ("01", true)] {
            let (mut body, pulls, cancelled) = TrackingBody::new(vec![1], 1);
            let error =
                read_request_bytes_with_limit(Some(&mut body), Some(declared), 1_024).unwrap_err();
            assert_eq!(
                matches!(error, BrokerBodyLimitError::InvalidContentLength),
                expected_invalid
            );
            assert_eq!(pulls.load(Ordering::Relaxed), 0);
            assert!(cancelled.load(Ordering::Relaxed));
        }
    }

    #[test]
    fn aborts_streams_that_hide_oversized_bodies() {
        let (mut body, _, cancelled) = TrackingBody::new(vec![b'x'; 2 * 1_024 * 1_024], 64 * 1_024);
        assert!(matches!(
            read_request_text_with_limit(Some(&mut body), None, 256 * 1_024),
            Err(BrokerBodyLimitError::PayloadTooLarge { .. })
        ));
        assert!(cancelled.load(Ordering::Relaxed));
    }

    #[test]
    fn rejects_full_aggregate_budget_before_pulling() {
        let budget = ResourceBudget::new(4, "http");
        let occupied = budget.reserve(4).unwrap();
        let (mut body, pulls, cancelled) = TrackingBody::new(vec![1], 1);
        assert!(matches!(
            read_request_bytes_with_reservation(Some(&mut body), None, 4, Some(&budget)),
            Err(BrokerBodyLimitError::Capacity(_))
        ));
        assert_eq!(pulls.load(Ordering::Relaxed), 0);
        assert!(cancelled.load(Ordering::Relaxed));
        assert_eq!(budget.used(), 4);
        occupied.release();
    }

    #[test]
    fn charges_source_plus_copy_peak_and_transfers_retained_capacity() {
        let budget = ResourceBudget::new(8, "http");
        let (mut body, _, _) = TrackingBody::new("éé".as_bytes().to_vec(), 4);
        let mut read =
            read_request_bytes_with_reservation(Some(&mut body), Some("1"), 4, Some(&budget))
                .unwrap();
        assert_eq!(read.bytes, "éé".as_bytes());
        assert_eq!(budget.used(), 4);
        assert!(budget.try_reserve(5).is_none());
        read.reservation.release();
        read.reservation.release();
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn rolls_back_failed_copy_peak_for_reuse() {
        let budget = ResourceBudget::new(7, "http");
        let (mut body, _, _) = TrackingBody::new(b"1234".to_vec(), 4);
        assert!(matches!(
            read_request_bytes_with_reservation(Some(&mut body), None, 4, Some(&budget)),
            Err(BrokerBodyLimitError::Capacity(_))
        ));
        assert_eq!(budget.used(), 0);
        budget.reserve(7).unwrap().release();
    }

    #[test]
    fn returns_exact_bytes_and_strictly_rejects_malformed_utf8() {
        let bytes = vec![0x7b, 0xc0, 0xaf, 0x7d];
        let (mut raw, _, _) = TrackingBody::new(bytes.clone(), 4);
        assert_eq!(
            read_request_bytes_with_limit(Some(&mut raw), None, 1_024).unwrap(),
            bytes
        );
        let (mut text, _, _) = TrackingBody::new(bytes, 4);
        assert!(matches!(
            read_request_text_with_limit(Some(&mut text), None, 1_024),
            Err(BrokerBodyLimitError::InvalidUtf8(_))
        ));
        assert_eq!(read_request_text_with_limit(None, None, 1_024).unwrap(), "");
    }

    #[test]
    fn bounds_responses_and_holds_capacity_until_body_transfer_or_cancel() {
        let budget = ResourceBudget::new(8, "response");
        let (body, _, _) = TrackingBody::new(b"1234".to_vec(), 4);
        let response =
            bound_http_response(streaming_response(body, None), 4, Some(&budget)).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(budget.used(), 4);
        assert_eq!(response.body.unwrap().read_all().unwrap(), b"1234");
        assert_eq!(budget.used(), 0);

        let (body, _, _) = TrackingBody::new(b"1234".to_vec(), 4);
        let mut response =
            bound_http_response(streaming_response(body, None), 4, Some(&budget)).unwrap();
        assert_eq!(budget.used(), 4);
        response.body.as_mut().unwrap().cancel().unwrap();
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn response_capacity_and_declared_oversize_fail_as_service_unavailable() {
        let budget = ResourceBudget::new(4, "response");
        let occupied = budget.reserve(4).unwrap();
        let (body, pulls, cancelled) = TrackingBody::new(vec![1], 1);
        let response =
            bound_http_response(streaming_response(body, None), 4, Some(&budget)).unwrap();
        assert_eq!(response.status, 503);
        assert_eq!(pulls.load(Ordering::Relaxed), 0);
        assert!(cancelled.load(Ordering::Relaxed));
        occupied.release();

        let (body, pulls, cancelled) = TrackingBody::new(vec![1], 1);
        let response = bound_http_response(streaming_response(body, Some("5")), 4, None).unwrap();
        assert_eq!(response.status, 503);
        assert_eq!(pulls.load(Ordering::Relaxed), 0);
        assert!(cancelled.load(Ordering::Relaxed));
    }

    #[test]
    fn rolls_back_failed_response_copy_peak_for_reuse() {
        let budget = ResourceBudget::new(7, "response");
        let (body, _, _) = TrackingBody::new(b"1234".to_vec(), 4);
        let response =
            bound_http_response(streaming_response(body, None), 4, Some(&budget)).unwrap();
        assert_eq!(response.status, 503);
        assert_eq!(budget.used(), 0);
        budget.reserve(7).unwrap().release();
    }

    #[test]
    fn leaves_event_stream_responses_unbuffered() {
        let budget = ResourceBudget::new(1, "response");
        let (body, pulls, _) = TrackingBody::new(b"data: ok\n\n".to_vec(), 10);
        let mut response = streaming_response(body, None);
        response.headers.insert(
            "Content-Type".into(),
            "text/event-stream; charset=utf-8".into(),
        );
        let response = bound_http_response(response, 1, Some(&budget)).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(pulls.load(Ordering::Relaxed), 0);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn counts_multibyte_and_replacement_sequences_without_allocating() {
        assert_eq!(utf8_byte_length("abc"), 3);
        assert_eq!(utf8_byte_length("é"), 2);
        assert_eq!(utf8_byte_length("😀"), 4);
        assert_eq!(utf8_byte_length_utf16(&[0xd800]), 3);
        assert_eq!(utf8_byte_length_utf16(&[0xdc00]), 3);
        assert_eq!(utf8_byte_length_utf16(&[0xd83d, 0xde00]), 4);
    }
}
