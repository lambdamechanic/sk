use serde_json::Value;
use std::fs;

#[path = "support/mod.rs"]
mod support;

use support::CliFixture;

#[test]
fn repo_add_and_catalog_lists_skills() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);
    let remote = fx.create_remote("catalog-repo", ".", "demo-skill");
    let url = remote.file_url();

    fx.sk_success(&["repo", "add", &url, "--alias", "demo"]);

    let lock_text = fs::read_to_string(fx.project.join("skills.lock.json")).unwrap();
    let lock: Value = serde_json::from_str(&lock_text).unwrap();
    let repos = lock["repos"]["entries"]
        .as_array()
        .expect("lockfile repos array");
    assert!(
        repos.iter().any(|entry| entry["alias"] == "demo"),
        "expected 'demo' alias in lockfile repos: {:?}",
        repos
    );

    let catalog = fx.run_json(&["repo", "catalog", "demo", "--json"]);
    let entries = catalog.as_array().expect("catalog json array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["name"], "demo-skill");

    let list = fx.run_json(&["repo", "list", "--json"]);
    let repos = list.as_array().expect("repo list array");
    assert_eq!(repos[0]["alias"], "demo");
    assert_eq!(repos[0]["spec"]["repo"], "catalog-repo");
}

#[test]
fn repo_catalog_accepts_direct_repo_input() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);
    let remote = fx.create_remote("direct-repo", ".", "direct-skill");

    let catalog = fx.run_json(&["repo", "catalog", &remote.file_url(), "--json"]);
    assert_eq!(catalog[0]["name"], "direct-skill");
}

#[test]
fn repo_search_matches_across_cached_repos() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);
    let alpha = fx.create_remote("alpha-repo", ".", "alpha-skill");
    let beta = fx.create_remote("beta-repo", ".", "beta-skill");

    fx.sk_success(&["repo", "add", &alpha.file_url(), "--alias", "alpha"]);
    fx.sk_success(&["repo", "add", &beta.file_url(), "--alias", "beta"]);

    let hits = fx.run_json(&["repo", "search", "beta", "--json"]);
    let array = hits.as_array().expect("search hits array");
    assert_eq!(array.len(), 1);
    assert_eq!(array[0]["repo"], "beta");
    assert_eq!(array[0]["name"], "beta-skill");
}

#[test]
fn repo_search_accepts_repo_flag() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);
    let remote = fx.create_remote("gamma-repo", ".", "gamma-skill");

    let hits = fx.run_json(&[
        "repo",
        "search",
        "gamma",
        "--repo",
        &remote.file_url(),
        "--json",
    ]);
    assert_eq!(hits[0]["name"], "gamma-skill");
}

#[test]
fn repo_list_marks_dirty_when_remote_unreachable() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);
    let remote = fx.create_remote("offline-repo", ".", "offline-skill");

    fx.sk_success(&["repo", "add", &remote.file_url(), "--alias", "offline"]);
    let first = fx.sk_cmd().args(["repo", "list"]).output().unwrap();
    assert!(first.status.success(), "initial repo list failed");
    let first_out = String::from_utf8_lossy(&first.stdout);
    assert!(
        !first_out.contains('*'),
        "fresh listing should not mark dirty"
    );

    std::fs::remove_dir_all(&remote.bare).unwrap();

    let offline = fx.sk_cmd().args(["repo", "list"]).output().unwrap();
    assert!(
        offline.status.success(),
        "repo list should not fail when fetch errors"
    );
    let offline_out = String::from_utf8_lossy(&offline.stdout);
    assert!(
        offline_out
            .lines()
            .any(|line| line.contains("offline") && line.contains('*')),
        "expected dirty flag next to offline repo counts\n{offline_out}"
    );
    assert!(
        offline_out.contains("stale cache"),
        "expected stale cache legend in output\n{offline_out}"
    );
}

#[test]
fn repo_remove_drops_registered_alias() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);
    let remote = fx.create_remote("remove-repo", ".", "remove-skill");

    fx.sk_success(&["repo", "add", &remote.file_url(), "--alias", "remove-me"]);
    fx.sk_success(&["repo", "remove", "remove-me"]);

    let lock_text = fs::read_to_string(fx.project.join("skills.lock.json")).unwrap();
    let lock: Value = serde_json::from_str(&lock_text).unwrap();
    let repos = lock["repos"]["entries"]
        .as_array()
        .expect("repos array after removal");
    assert!(
        repos.iter().all(|entry| entry["alias"] != "remove-me"),
        "lockfile should not list alias after removal: {:?}",
        repos
    );
}

#[test]
fn repo_remove_supports_json_and_repo_specs() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);
    let remote = fx.create_remote("json-remove", ".", "json-skill");
    let url = remote.file_url();

    fx.sk_success(&["repo", "add", &url, "--alias", "json"]);

    let removed = fx.run_json(&["repo", "remove", &url, "--json"]);
    assert_eq!(removed["status"], "removed");
    assert_eq!(removed["alias"], "json");

    let missing = fx.run_json(&["repo", "remove", "json", "--json"]);
    assert_eq!(missing["status"], "not_found");
    assert_eq!(missing["target"], "json");
}
