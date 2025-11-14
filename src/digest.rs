use anyhow::Result;
use sha2::{Digest as _, Sha256};
use std::borrow::Cow;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

pub fn digest_dir(dir: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut files: Vec<_> = WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| !should_ignore(e.path()))
        .map(|e| e.into_path())
        .collect();
    files.sort();
    for path in files {
        let rel = path.strip_prefix(dir).unwrap_or(&path);
        hasher.update(rel.to_string_lossy().as_bytes());
        let data = fs::read(&path)?;
        let normalized = normalize_crlf(&data);
        hasher.update(&normalized);
    }
    let hex = format!("sha256:{:x}", hasher.finalize());
    Ok(hex)
}

fn should_ignore(p: &Path) -> bool {
    let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
    name == ".DS_Store" || name.ends_with("~") || name.ends_with(".swp") || name == ".git"
}

fn normalize_crlf(input: &[u8]) -> Cow<'_, [u8]> {
    if looks_binary(input) || !contains_crlf(input) {
        return Cow::Borrowed(input);
    }
    let mut out = Vec::with_capacity(input.len());
    let mut idx = 0;
    while idx < input.len() {
        if idx + 1 < input.len() && input[idx] == b'\r' && input[idx + 1] == b'\n' {
            out.push(b'\n');
            idx += 2;
        } else {
            out.push(input[idx]);
            idx += 1;
        }
    }
    Cow::Owned(out)
}

fn looks_binary(data: &[u8]) -> bool {
    data.contains(&0)
}

fn contains_crlf(data: &[u8]) -> bool {
    data.windows(2).any(|w| matches!(w, b"\r\n"))
}
