use std::str;

#[path = "support/mod.rs"]
mod support;

use support::CliFixture;

#[test]
fn list_prints_repo_and_description() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let remote = fx.create_remote("skills-demo", "demo", "demo");
    let repo_url = remote.file_url();
    let mut install_args = vec!["install", &repo_url, "demo"];
    if remote.skill_path() != "." {
        install_args.push("--path");
        install_args.push(remote.skill_path());
    }
    fx.sk_success(&install_args);

    let out = fx.sk_cmd().args(["list"]).output().unwrap();
    assert!(out.status.success(), "list failed: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "expected one installed skill");
    let cols: Vec<&str> = lines[0].split('\t').collect();
    assert_eq!(cols.len(), 4, "expected 4 columns");
    assert_eq!(cols[0], "demo");
    assert_eq!(cols[1], format!("{}:{}", repo_url, remote.skill_path()));
    assert_eq!(cols[2], "demo");
    assert_eq!(cols[3], "fixture");

    let json = fx.run_json(&["list", "--json"]);
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    let entry = &arr[0];
    assert_eq!(entry["installName"], "demo");
    assert_eq!(entry["repo"], format!("{}:{}", repo_url, remote.skill_path()));
    assert_eq!(entry["skillPath"], "demo");
    assert_eq!(entry["description"], "fixture");
}
