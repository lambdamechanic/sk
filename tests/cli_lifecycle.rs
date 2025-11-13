use std::fs;

#[path = "support/mod.rs"]
mod support;

use support::{normalize_newlines, parse_status_entries, CliFixture, StatusEntry};

#[test]
fn lifecycle_install_update_upgrade_and_remove_flow() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let remote = fx.create_remote("repo-lifecycle", "skills/sample", "sample");
    fx.install_from_remote(&remote, "sample");

    let status = current_status(&fx);
    assert_eq!(status[0].state, "clean");
    assert!(status[0].update.is_none());

    // Remote advances to v2; cache-only update should surface via status.
    let v2 = remote.overwrite_file("file.txt", "v2\n", "v2");

    // Without cache refresh, status remains unaware of updates.
    let pre_update = current_status(&fx);
    assert!(pre_update[0].update.is_none());

    fx.sk_success(&["update"]);

    let post_update = current_status(&fx);
    let update_str = post_update[0]
        .update
        .as_ref()
        .expect("status reports remote update");
    let lock = fx.lock_json();
    let locked_commit = lock["skills"][0]["commit"].as_str().unwrap();
    assert!(
        update_str.contains(&locked_commit[..7]) && update_str.contains(&v2[..7]),
        "status shows locked and new tip: {update_str}"
    );

    fx.sk_success(&["upgrade", "sample"]);

    let skill_dir = fx.skill_dir("sample");
    let contents = fs::read_to_string(skill_dir.join("file.txt")).unwrap();
    assert_eq!(normalize_newlines(&contents), "v2\n");

    let lock_after = fx.lock_json();
    let new_commit = lock_after["skills"][0]["commit"]
        .as_str()
        .expect("commit present");
    assert_eq!(new_commit, v2, "lockfile tracks upgraded commit");
    let digest_value = lock_after["skills"][0]["digest"]
        .as_str()
        .expect("digest present");
    let recomputed = sk::digest::digest_dir(&skill_dir).unwrap();
    assert_eq!(digest_value, recomputed, "digest recomputed on upgrade");

    let status_clean = current_status(&fx);
    assert_eq!(status_clean[0].state, "clean");
    assert!(status_clean[0].update.is_none());
    assert_eq!(status_clean[0].locked, status_clean[0].current);

    fx.sk_success(&["remove", "sample"]);
    assert!(
        !skill_dir.exists(),
        "remove deletes the installed skill directory"
    );
    let lock_after_remove = fx.lock_json();
    assert!(
        lock_after_remove["skills"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false),
        "lockfile entry dropped after remove"
    );
}

fn current_status(fx: &CliFixture) -> Vec<StatusEntry> {
    parse_status_entries(fx.run_json(&["status", "--json"]))
}
