use std::fs;
use std::path::Path;
use std::process::Command;

use sk::skills::list_skills_in_repo;

fn write_skill(repo: &Path, subdir: &str, name: &str) {
    let dir = repo.join(subdir);
    fs::create_dir_all(&dir).unwrap();
    let skill_md = dir.join("SKILL.md");
    let content = format!(
        "---\nname: {name}\ndescription: {name} desc\n---\n\nBody\n"
    );
    fs::write(&skill_md, content).unwrap();
}

#[test]
fn list_skills_finds_multiple_entries() {
    let repo = tempfile::tempdir().unwrap();
    let repo_path = repo.path();

    // init repo
    assert!(Command::new("git")
        .args(["-C", repo_path.to_str().unwrap(), "init"])
        .status()
        .unwrap()
        .success());
    // minimal identity for commit
    let _ = Command::new("git")
        .args([
            "-C",
            repo_path.to_str().unwrap(),
            "config",
            "user.email",
            "you@example.com",
        ])
        .status();
    let _ = Command::new("git")
        .args([
            "-C",
            repo_path.to_str().unwrap(),
            "config",
            "user.name",
            "You",
        ])
        .status();

    write_skill(repo_path, "skills/a", "a");
    write_skill(repo_path, "skills/b", "b");

    assert!(Command::new("git")
        .args(["-C", repo_path.to_str().unwrap(), "add", "."])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args([
            "-C",
            repo_path.to_str().unwrap(),
            "commit",
            "-m",
            "add skills"
        ])
        .status()
        .unwrap()
        .success());

    let skills = list_skills_in_repo(repo_path, "HEAD").expect("list skills");
    let mut names: Vec<_> = skills.iter().map(|s| s.meta.name.clone()).collect();
    names.sort();
    assert_eq!(names, vec!["a", "b"]);

    let mut paths: Vec<_> = skills.iter().map(|s| s.skill_path.clone()).collect();
    paths.sort();
    assert_eq!(paths, vec!["skills/a", "skills/b"]);
}
