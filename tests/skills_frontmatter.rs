use std::fs;
use std::path::PathBuf;

use sk::skills::parse_frontmatter_file;

#[test]
fn parse_frontmatter_file_ok() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("SKILL.md");
    let content = r#"---
name: demo
description: Demo skill
---

Some content here.
"#;
    fs::write(&p, content).unwrap();

    let meta = parse_frontmatter_file(&p).expect("should parse valid frontmatter");
    assert_eq!(meta.name, "demo");
    assert_eq!(meta.description, "Demo skill");
}

#[test]
fn parse_frontmatter_file_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("SKILL.md");
    fs::write(&p, "no frontmatter here").unwrap();

    let err = parse_frontmatter_file(&p).err().expect("expected error");
    let msg = format!("{}", err);
    assert!(msg.contains("front-matter"), "unexpected error: {}", msg);
}
