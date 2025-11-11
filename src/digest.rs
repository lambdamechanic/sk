use anyhow::Result;
use sha2::{Digest as _, Sha256};
use walkdir::WalkDir;
use std::fs;
use std::path::Path;

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
        hasher.update(&data);
    }
    let hex = format!("sha256:{:x}", hasher.finalize());
    Ok(hex)
}

fn should_ignore(p: &Path) -> bool {
    let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
    name == ".DS_Store" || name.ends_with("~") || name.ends_with(".swp") || name == ".git"
}

