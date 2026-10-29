//! File name helpers for downloaded tracks.

use std::path::{Path, PathBuf};

const MAX_STEM_CHARS: usize = 180;

/// Device names Windows reserves, with or without an extension (`CON`, `nul.txt`, ...).
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Replace characters that are unsafe in file names on common file systems, and
/// avoid names Windows rejects (reserved device names, trailing dots or spaces).
/// Applied on every platform so files stay valid when copied to Windows.
pub fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    let truncated: String = trimmed.chars().take(MAX_STEM_CHARS).collect();
    // Truncation can leave a trailing dot or space again.
    let mut out = truncated.trim_end_matches(['.', ' ']).to_owned();
    if out.is_empty() {
        out.push_str("untitled");
    }
    let base_len = out.find('.').unwrap_or(out.len());
    let base = out[..base_len].trim_end();
    if WINDOWS_RESERVED.iter().any(|r| r.eq_ignore_ascii_case(base)) {
        out.insert(base.len(), '_');
    }
    out
}

/// `<Artist> - <Title>` file stem.
pub fn track_stem(artist: &str, title: &str) -> String {
    sanitize(&format!("{artist} - {title}"))
}

/// Return `dir/stem.ext`, or `dir/stem (n).ext` with the lowest free `n` when the file exists.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let candidate = dir.join(format!("{stem}.{ext}"));
    if !candidate.exists() {
        return candidate;
    }
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .expect("infinite iterator")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_unsafe_characters() {
        assert_eq!(sanitize("AC/DC: Back?In*Black"), "AC_DC_ Back_In_Black");
        assert_eq!(sanitize("a\"b<c>d|e\\f"), "a_b_c_d_e_f");
        assert_eq!(sanitize("tab\there"), "tab_here");
    }

    #[test]
    fn trims_dots_and_whitespace() {
        assert_eq!(sanitize("  ..hidden.  "), "hidden");
        assert_eq!(sanitize("..."), "untitled");
        assert_eq!(sanitize(""), "untitled");
    }

    #[test]
    fn limits_length() {
        let long = "x".repeat(500);
        assert_eq!(sanitize(&long).chars().count(), MAX_STEM_CHARS);
    }

    #[test]
    fn avoids_windows_reserved_names() {
        assert_eq!(sanitize("CON"), "CON_");
        assert_eq!(sanitize("con"), "con_");
        assert_eq!(sanitize("nul.m4a"), "nul_.m4a");
        assert_eq!(sanitize("Aux.live"), "Aux_.live");
        assert_eq!(sanitize("COM1"), "COM1_");
        assert_eq!(sanitize("lpt9.x"), "lpt9_.x");
        // Not reserved: longer names, COM0/COM10, reserved word inside a name.
        assert_eq!(sanitize("CONTROL"), "CONTROL");
        assert_eq!(sanitize("COM10"), "COM10");
        assert_eq!(sanitize("COM0"), "COM0");
        assert_eq!(sanitize("Con Funk Shun - Love's Train"), "Con Funk Shun - Love's Train");
        assert_eq!(track_stem("Blender Foundation", "Big Buck Bunny"), "Blender Foundation - Big Buck Bunny");
    }

    #[test]
    fn truncation_leaves_no_trailing_dot_or_space() {
        let name = format!("{} . x", "a".repeat(MAX_STEM_CHARS - 2));
        let out = sanitize(&name);
        assert!(!out.ends_with(['.', ' ']), "{out:?}");
    }

    #[test]
    fn stem_format() {
        assert_eq!(track_stem("Blender Foundation", "Big Buck Bunny"), "Blender Foundation - Big Buck Bunny");
    }

    #[test]
    fn collision_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let first = unique_path(dir.path(), "A - B", "m4a");
        assert_eq!(first, dir.path().join("A - B.m4a"));
        std::fs::write(&first, b"x").unwrap();
        let second = unique_path(dir.path(), "A - B", "m4a");
        assert_eq!(second, dir.path().join("A - B (2).m4a"));
        std::fs::write(&second, b"x").unwrap();
        assert_eq!(unique_path(dir.path(), "A - B", "m4a"), dir.path().join("A - B (3).m4a"));
    }
}
