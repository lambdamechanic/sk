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
    let out = cmd
        .current_dir(&project)
        .args(["precommit"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "precommit should pass for remote sources"
    );
}

#[test]
fn precommit_treats_localhost_exact_only() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // localhost subdomain should NOT be treated as local
    let body = r#"{
  "version": 1,
  "skills": [
    {"installName":"s0","source":{"url":"https://localhost.example.com/o/r.git","host":"localhost.example.com","owner":"o","repo":"r","skillPath":"skill"},"ref":null,"commit":"deadbeef","digest":"sha256:abc","installedAt":"1970-01-01T00:00:00Z"}
  ],
  "generatedAt":"1970-01-01T00:00:00Z"
}
"#;
    fs::write(project.join("skills.lock.json"), body).unwrap();
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .args(["precommit"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "precommit should pass for localhost subdomain"
    );
}

#[test]
fn precommit_flags_http_localhost_and_ssh_localhost() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // Two entries: https://localhost and scp-like git@localhost:
    let body = r#"{
  "version": 1,
  "skills": [
    {"installName":"a","source":{"url":"https://localhost/o/r.git","host":"localhost","owner":"o","repo":"r","skillPath":"skill"},"ref":null,"commit":"deadbeef","digest":"sha256:abc","installedAt":"1970-01-01T00:00:00Z"},
    {"installName":"b","source":{"url":"git@localhost:o/r.git","host":"localhost","owner":"o","repo":"r","skillPath":"skill"},"ref":null,"commit":"deadbeef","digest":"sha256:def","installedAt":"1970-01-01T00:00:00Z"}
  ],
  "generatedAt":"1970-01-01T00:00:00Z"
}
"#;
    fs::write(project.join("skills.lock.json"), body).unwrap();
    let mut cmd = cargo_bin_cmd!("sk");
    let assert = cmd
        .current_dir(&project)
        .args(["precommit"])
        .assert()
        .failure();
    assert.stderr(predicates::str::contains(
        "local (file:// or localhost) sources",
    ));
}

#[test]
fn precommit_flags_scp_with_non_git_user_and_ipv6() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // Two entries: non-git user@localhost and IPv6 loopback
    let body = r#"{
  "version": 1,
  "skills": [
    {"installName":"a","source":{"url":"me@localhost:o/r.git","host":"localhost","owner":"o","repo":"r","skillPath":"skill"},"ref":null,"commit":"deadbeef","digest":"sha256:abc","installedAt":"1970-01-01T00:00:00Z"},
    {"installName":"b","source":{"url":"user@[::1]:o/r.git","host":"[::1]","owner":"o","repo":"r","skillPath":"skill"},"ref":null,"commit":"deadbeef","digest":"sha256:def","installedAt":"1970-01-01T00:00:00Z"}
  ],
  "generatedAt":"1970-01-01T00:00:00Z"
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
fn precommit_flags_ssh_with_userinfo_and_ipv6() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // ssh:// with user@localhost and user@[::1]
    let body = r#"{
  "version": 1,
  "skills": [
    {"installName":"a","source":{"url":"ssh://me@localhost/o/r.git","host":"localhost","owner":"o","repo":"r","skillPath":"skill"},"ref":null,"commit":"deadbeef","digest":"sha256:abc","installedAt":"1970-01-01T00:00:00Z"},
    {"installName":"b","source":{"url":"ssh://user@[::1]:2222/o/r.git","host":"[::1]","owner":"o","repo":"r","skillPath":"skill"},"ref":null,"commit":"deadbeef","digest":"sha256:def","installedAt":"1970-01-01T00:00:00Z"}
  ],
  "generatedAt":"1970-01-01T00:00:00Z"
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
fn precommit_flags_scp_without_userinfo() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // scp-style without userinfo: localhost:o/r.git and [::1]:o/r.git
    let body = r#"{
  "version": 1,
  "skills": [
    {"installName":"a","source":{"url":"localhost/o/r.git","host":"localhost","owner":"o","repo":"r","skillPath":"skill"},"ref":null,"commit":"deadbeef","digest":"sha256:abc","installedAt":"1970-01-01T00:00:00Z"},
    {"installName":"b","source":{"url":"[::1]:o/r.git","host":"[::1]","owner":"o","repo":"r","skillPath":"skill"},"ref":null,"commit":"deadbeef","digest":"sha256:def","installedAt":"1970-01-01T00:00:00Z"}
  ],
  "generatedAt":"1970-01-01T00:00:00Z"
}
"#;
    fs::write(project.join("skills.lock.json"), body).unwrap();
    let mut cmd = cargo_bin_cmd!("sk");
    cmd.current_dir(&project).args(["precommit"]);
    cmd.assert()
        .failure()
        .stderr(contains("local (file:// or localhost) sources"));
}
