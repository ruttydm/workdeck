//! Compatibility-path normalization at VCS process boundaries.

/// Normalize compatibility-layer paths into native paths for the current OS.
pub fn normalize_path_for_os(path: &str) -> String {
    if cfg!(windows) {
        normalize_path_for_platform(path, "win32")
    } else {
        path.to_owned()
    }
}

/// Platform-explicit form used by provider probes and cross-platform tests.
pub fn normalize_path_for_platform(path: &str, platform: &str) -> String {
    if platform != "win32" {
        return path.to_owned();
    }
    let normalized = slash_drive_path(path)
        .or_else(|| compatibility_drive_path(path, "/cygdrive/"))
        .or_else(|| compatibility_drive_path(path, "/mnt/"))
        .or_else(|| compatibility_drive_path(path, "/"));
    normalized.map_or_else(|| path.to_owned(), |path| path.replace('/', "\\"))
}

fn slash_drive_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix('/')?;
    let drive = rest.as_bytes().first().copied()?;
    if !drive.is_ascii_alphabetic() || rest.as_bytes().get(1) != Some(&b':') {
        return None;
    }
    finish_drive_path(drive, &rest[2..])
}

fn compatibility_drive_path(path: &str, prefix: &str) -> Option<String> {
    let rest = path.strip_prefix(prefix)?;
    let drive = rest.as_bytes().first().copied()?;
    if !drive.is_ascii_alphabetic() {
        return None;
    }
    finish_drive_path(drive, &rest[1..])
}

fn finish_drive_path(drive: u8, tail: &str) -> Option<String> {
    let remainder = match tail.as_bytes().first().copied() {
        None => "",
        Some(b'/' | b'\\') => &tail[1..],
        Some(_) => return None,
    };
    Some(format!(
        "{}:/{}",
        char::from(drive.to_ascii_uppercase()),
        remainder
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_unix_style_windows_paths_for_native_processes() {
        assert_eq!(
            normalize_path_for_platform("/cygdrive/c/work/repo", "win32"),
            "C:\\work\\repo"
        );
        assert_eq!(
            normalize_path_for_platform("/c/work/repo", "win32"),
            "C:\\work\\repo"
        );
        assert_eq!(
            normalize_path_for_platform("/mnt/c/work/repo", "win32"),
            "C:\\work\\repo"
        );
        assert_eq!(
            normalize_path_for_platform("/c:/work/repo", "win32"),
            "C:\\work\\repo"
        );
        assert_eq!(
            normalize_path_for_platform("/home/project", "win32"),
            "/home/project"
        );
        assert_eq!(
            normalize_path_for_platform("C:\\work\\repo", "win32"),
            "C:\\work\\repo"
        );
        assert_eq!(
            normalize_path_for_platform("C:/work/repo", "win32"),
            "C:/work/repo"
        );
    }

    #[test]
    fn leaves_paths_unchanged_on_unix_like_platforms() {
        for path in [
            "/cygdrive/c/work/repo",
            "/c/work/repo",
            "/mnt/c/work/repo",
            "/c:/work/repo",
            "/home/project",
            "/Users/project",
            "relative/path",
            "C:\\work\\repo",
            "C:/work/repo",
        ] {
            assert_eq!(normalize_path_for_platform(path, "linux"), path);
            assert_eq!(normalize_path_for_platform(path, "darwin"), path);
        }
    }
}
