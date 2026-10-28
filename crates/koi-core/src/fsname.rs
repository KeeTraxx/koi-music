//! File name helpers for downloaded tracks.

use std::path::{Path, PathBuf};

const MAX_STEM_CHARS: usize = 180;

/// Replace characters that are unsafe in file names on common file systems.
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
    let mut out: String = trimmed.chars().take(MAX_STEM_CHARS).collect();
    if out.is_empty() {
        out.push_str("untitled");
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
    fn stem_format() {
        assert_eq!(track_stem("Daft Punk", "Around the World"), "Daft Punk - Around the World");
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
