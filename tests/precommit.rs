use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn git(args: &[&str], cwd: &Path) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "git {:?} failed in {}",
        args,
        cwd.display()
    );
}

#[test]
fn precommit_fails_on_local_file_sources() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // Lockfile with a local file URL
    let body = r#"{
  "version": 1,
  "skills": [
    {
      "installName": "s0",
      "source": {
        "url": "file:///tmp/local/repo.git",
        "host": "local",
        "owner": "tmp",
        "repo": "repo",
        "skillPath": "skill"
      },
      "ref": null,
      "commit": "deadbeef",
      "digest": "sha256:abc",
      "installedAt": "1970-01-01T00:00:00Z"
    }
  ],
  "generatedAt": "1970-01-01T00:00:00Z"
}
"#;
    fs::write(project.join("skills.lock.json"), body).unwrap();

    let mut cmd = cargo_bin_cmd!("sk");
    cmd.current_dir(&project).args(["precommit"]);
    cmd.assert()
        .failure()
        .stderr(contains("local (file:// or localhost) sources"));
}

#[test]
fn precommit_passes_on_remote_sources() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let body = r#"{
  "version": 1,
  "skills": [
    {
      "installName": "s0",
      "source": {
        "url": "git@github.com:o/r.git",
        "host": "github.com",
        "owner": "o",
        "repo": "r",
        "skillPath": "skill"
      },
      "ref": null,
      "commit": "deadbeef",
      "digest": "sha256:abc",
      "installedAt": "1970-01-01T00:00:00Z"
    }
  ],
  "generatedAt": "1970-01-01T00:00:00Z"
}
"#;
    fs::write(project.join("skills.lock.json"), body).unwrap();

    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd.current_dir(&project).args(["precommit"]).output().unwrap();
    assert!(out.status.success(), "precommit should pass for remote sources");
}

