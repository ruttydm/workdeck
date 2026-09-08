//! Bounded newline-delimited native protocol input.

use std::io::{self, BufRead, Read};
use workdeck_extension_api::MAX_MESSAGE_BYTES;

pub(crate) fn read_protocol_frame(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let count = reader
        .take((MAX_MESSAGE_BYTES + 2) as u64)
        .read_until(b'\n', &mut bytes)?;
    if count == 0 {
        return Ok(None);
    }
    if bytes.last() != Some(&b'\n') || count > MAX_MESSAGE_BYTES + 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unterminated or oversized extension protocol frame",
        ));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_frames_and_accepts_exact_payload_limit() {
        let mut reader = io::Cursor::new(b"first\r\nsecond\n");
        assert_eq!(
            read_protocol_frame(&mut reader).unwrap(),
            Some("first\r\n".into())
        );
        assert_eq!(
            read_protocol_frame(&mut reader).unwrap(),
            Some("second\n".into())
        );
        assert_eq!(read_protocol_frame(&mut reader).unwrap(), None);
        let mut frame = vec![b'x'; MAX_MESSAGE_BYTES];
        frame.push(b'\n');
        assert_eq!(
            read_protocol_frame(&mut io::Cursor::new(frame))
                .unwrap()
                .unwrap()
                .len(),
            MAX_MESSAGE_BYTES + 1
        );
    }

    #[test]
    fn rejects_invalid_utf8_unterminated_and_oversized_frames_with_bounded_consumption() {
        for frame in [vec![0xff, b'\n'], b"unterminated".to_vec()] {
            assert_eq!(
                read_protocol_frame(&mut io::Cursor::new(frame))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData
            );
        }
        let mut reader = io::Cursor::new(vec![b'x'; MAX_MESSAGE_BYTES * 2]);
        assert_eq!(
            read_protocol_frame(&mut reader).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(reader.position(), (MAX_MESSAGE_BYTES + 2) as u64);
    }
}
