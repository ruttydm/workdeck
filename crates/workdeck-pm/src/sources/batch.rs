use super::{GitOid, SourceCaptureLimits, git::BoundGit};
use crate::{ErrorCode, PmError, Result};

pub(super) fn read(
    git: &BoundGit,
    oids: &[GitOid],
    limits: &SourceCaptureLimits,
) -> Result<Vec<Vec<u8>>> {
    if oids.is_empty() {
        return Ok(Vec::new());
    }
    if oids.len() > limits.max_entries {
        return Err(invalid("Git batch exceeds entry limit"));
    }
    let input = oids
        .iter()
        .map(|oid| format!("{oid}\n"))
        .collect::<String>()
        .into_bytes();
    // Exact OIDs only: no revision expressions, paths, shell or filters.
    let header_cap = oids
        .len()
        .checked_mul(96)
        .ok_or_else(|| invalid("Git batch framing limit overflow"))?;
    let headers = git.run(
        &["cat-file".into(), "--batch-check".into()],
        Some(input.clone()),
        None,
        header_cap,
    )?;
    if !headers.status.success() {
        return Err(invalid("Git could not inspect requested batch objects"));
    }
    let sizes = parse_sizes(&headers.stdout, oids, limits)?;
    let total = sizes.iter().try_fold(0usize, |sum, size| {
        sum.checked_add(*size)
            .ok_or_else(|| invalid("Git batch byte limit overflow"))
    })?;
    let output_cap = total
        .checked_add(header_cap)
        .ok_or_else(|| invalid("Git batch output limit overflow"))?;
    let output = git.run(
        &["cat-file".into(), "--batch".into()],
        Some(input),
        None,
        output_cap,
    )?;
    if !output.status.success() {
        return Err(invalid("Git could not read requested batch objects"));
    }
    parse_bodies(&output.stdout, oids, &sizes, limits)
}
fn header(line: &[u8], expected: &GitOid) -> Result<usize> {
    if line.len() > 94 {
        return Err(invalid("Git batch header exceeds framing bound"));
    }
    let line = std::str::from_utf8(line).map_err(|_| invalid("Git batch header must be ASCII"))?;
    let mut fields = line.split(' ');
    if fields.next() != Some(expected.as_str()) || fields.next() != Some("blob") {
        return Err(invalid(
            "Git batch returned unexpected object identity or type",
        ));
    }
    let size = fields
        .next()
        .ok_or_else(|| invalid("Git batch omitted object size"))?;
    if size.is_empty()
        || !size.bytes().all(|byte| byte.is_ascii_digit())
        || (size.len() > 1 && size.starts_with('0'))
        || fields.next().is_some()
    {
        return Err(invalid("Git batch returned invalid object size framing"));
    }
    size.parse()
        .map_err(|_| invalid("Git batch object size overflow"))
}
fn parse_sizes(bytes: &[u8], oids: &[GitOid], limits: &SourceCaptureLimits) -> Result<Vec<usize>> {
    let mut remaining = bytes;
    let mut total = 0usize;
    let mut sizes = Vec::with_capacity(oids.len());
    for oid in oids {
        let end = remaining
            .iter()
            .position(|&byte| byte == b'\n')
            .ok_or_else(|| invalid("Git batch header is truncated"))?;
        let size = header(&remaining[..end], oid)?;
        if size
            > limits
                .max_file_bytes
                .min(limits.max_total_bytes.saturating_sub(total))
        {
            return Err(invalid("Git blob exceeds source capture bound"));
        }
        total += size;
        sizes.push(size);
        remaining = &remaining[end + 1..];
    }
    if !remaining.is_empty() {
        return Err(invalid("Git batch returned unrequested object headers"));
    }
    Ok(sizes)
}
fn parse_bodies(
    bytes: &[u8],
    oids: &[GitOid],
    sizes: &[usize],
    limits: &SourceCaptureLimits,
) -> Result<Vec<Vec<u8>>> {
    if oids.len() != sizes.len() {
        return Err(invalid("Git batch size membership differs"));
    }
    let mut remaining = bytes;
    let mut result = Vec::with_capacity(oids.len());
    let mut total = 0usize;
    for (oid, &expected_size) in oids.iter().zip(sizes) {
        let end = remaining
            .iter()
            .position(|&byte| byte == b'\n')
            .ok_or_else(|| invalid("Git batch object header is truncated"))?;
        let size = header(&remaining[..end], oid)?;
        if size != expected_size
            || size
                > limits
                    .max_file_bytes
                    .min(limits.max_total_bytes.saturating_sub(total))
        {
            return Err(invalid(
                "Git batch object size changed or exceeds capture bound",
            ));
        }
        remaining = &remaining[end + 1..];
        if remaining.len() <= size || remaining[size] != b'\n' {
            return Err(invalid(
                "Git batch object body is truncated or lacks delimiter",
            ));
        }
        result.push(remaining[..size].to_vec());
        total += size;
        remaining = &remaining[size + 1..];
    }
    if !remaining.is_empty() {
        return Err(invalid(
            "Git batch returned trailing or unrequested object content",
        ));
    }
    Ok(result)
}
fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn oid() -> GitOid {
        "a".repeat(40).parse().unwrap()
    }
    #[test]
    fn batch_frames_preserve_binary_and_newlines_and_reject_truncation_and_identity_changes() {
        let oid = oid();
        let limits = SourceCaptureLimits::default();
        let mut bytes = format!("{oid} blob 4\n").into_bytes();
        bytes.extend_from_slice(b"\0\nx\xff\n");
        assert_eq!(
            parse_bodies(&bytes, std::slice::from_ref(&oid), &[4], &limits).unwrap(),
            vec![b"\0\nx\xff".to_vec()]
        );
        for length in 0..bytes.len() {
            assert!(
                parse_bodies(&bytes[..length], std::slice::from_ref(&oid), &[4], &limits).is_err(),
                "{length}"
            );
        }
        let mut wrong = bytes.clone();
        wrong[0] = b'b';
        assert!(parse_bodies(&wrong, std::slice::from_ref(&oid), &[4], &limits).is_err());
        let mut trailing = bytes.clone();
        trailing.push(b'x');
        assert!(parse_bodies(&trailing, std::slice::from_ref(&oid), &[4], &limits).is_err());
        assert!(parse_bodies(&bytes, std::slice::from_ref(&oid), &[3], &limits).is_err());
    }
    #[test]
    fn batch_headers_reject_nonblobs_per_file_total_missing_and_extra_objects() {
        let oid = oid();
        let limits = SourceCaptureLimits {
            max_file_bytes: 4,
            max_total_bytes: 6,
            ..Default::default()
        };
        let line = format!("{oid} blob 4\n");
        assert_eq!(
            parse_sizes(line.as_bytes(), std::slice::from_ref(&oid), &limits).unwrap(),
            vec![4]
        );
        for bad in [
            format!("{oid} tree 4\n"),
            format!("{oid} blob 5\n"),
            format!("{oid} blob 04\n"),
            format!("{oid} missing\n"),
            format!("{oid} blob 4\n{line}"),
            format!("{oid} blob 18446744073709551616\n"),
        ] {
            assert!(
                parse_sizes(bad.as_bytes(), std::slice::from_ref(&oid), &limits).is_err(),
                "{bad}"
            );
        }
        assert!(
            parse_sizes(
                format!("{line}{line}").as_bytes(),
                &[oid.clone(), oid],
                &limits
            )
            .is_err()
        );
    }
}
