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

    let registry = fx.project.join("skills.repos.json");
    let registry_contents = fs::read_to_string(&registry).unwrap();
    assert!(registry_contents.contains("\"demo\""));

    let catalog = fx.run_json(&["repo", "catalog", "demo", "--json"]);
    let entries = catalog.as_array().expect("catalog json array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["name"], "demo-skill");

    let list = fx.run_json(&["repo", "list", "--json"]);
    let repos = list.as_array().expect("repo list array");
    assert_eq!(repos[0]["alias"], "demo");
    assert_eq!(repos[0]["spec"]["owner"], "sources");
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
