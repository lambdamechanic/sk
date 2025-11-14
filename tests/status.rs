use serde_json::Value as Json;
use std::fs;

#[path = "support/mod.rs"]
mod support;

use support::CliFixture;

fn status_json(fix: &CliFixture) -> Json {
    fix.run_json(["status", "--json"].as_ref())
}

#[test]
fn status_reports_modified_after_local_edit() {
    let fix = CliFixture::new();
    let remote = fix.create_remote("status-clean", "skill", "sfile");
    fix.install_from_remote(&remote, "sfile");

    let status_clean = status_json(&fix);
    assert_eq!(status_clean.as_array().unwrap().len(), 1);
    assert_eq!(
        status_clean[0]["state"].as_str().unwrap(),
        "clean",
        "fresh install should be clean"
    );
    assert!(status_clean[0]["update"].is_null());
    let locked = status_clean[0]["locked"].as_str().unwrap().to_string();
    let current = status_clean[0]["current"].as_str().unwrap().to_string();
    assert_eq!(locked, current);

    // Local edit flips status to modified
    fs::write(fix.skill_dir("sfile").join("file.txt"), "local edit\n").unwrap();

    let status_modified = status_json(&fix);
    assert_eq!(status_modified.as_array().unwrap().len(), 1);
    assert_eq!(
        status_modified[0]["state"].as_str().unwrap(),
        "modified",
        "local edits must be reported"
    );
    let modified_digest = status_modified[0]["current"]
        .as_str()
        .expect("digest present for modified skill");
    assert_ne!(modified_digest, locked);
}

#[test]
fn status_reports_remote_update_after_cache_fetch() {
    let fix = CliFixture::new();
    let remote = fix.create_remote("status-update", "skill", "sfile");
    let v1 = remote.head();
    fix.install_from_remote(&remote, "sfile");

    let first_status = status_json(&fix);
    assert_eq!(first_status[0]["state"].as_str().unwrap(), "clean");
    assert!(first_status[0]["update"].is_null());

    // Advance remote repository to v2 and push
    fs::write(
        remote.work.join(remote.skill_path()).join("file.txt"),
        "v2\n",
    )
    .unwrap();
    support::git(&["add", "."], &remote.work);
    support::git(&["commit", "-m", "v2"], &remote.work);
    support::git(&["push", "origin", "main"], &remote.work);
    let v2 = remote.head();

    let out = fix.sk_cmd().args(["update"]).output().unwrap();
    assert!(out.status.success(), "sk update failed: {out:?}");

    let status_after_update = status_json(&fix);
    assert_eq!(
        status_after_update[0]["state"].as_str().unwrap(),
        "clean",
        "local tree still matches lock digest"
    );
    let update_str = status_after_update[0]["update"]
        .as_str()
        .expect("update string present after cache fetch");
    let expected = format!("{} -> {}", &v1[..7], &v2[..7]);
    assert_eq!(update_str, expected);
}
