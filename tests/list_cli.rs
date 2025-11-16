use std::str;

#[path = "support/mod.rs"]
mod support;

use support::CliFixture;

#[test]
fn list_prints_name_and_description() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let remote = fx.create_remote("skills-demo", "demo", "demo-fm");
    let repo_url = remote.file_url();
    let mut install_args = vec!["install", &repo_url, "demo-fm"];
    if remote.skill_path() != "." {
        install_args.push("--path");
        install_args.push(remote.skill_path());
    }
    install_args.push("--alias");
    install_args.push("demo");
    fx.sk_success(&install_args);

    let out = fx.sk_cmd().args(["list"]).output().unwrap();
    assert!(out.status.success(), "list failed: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "expected one installed skill");
    assert_eq!(lines[0], "demo-fm  fixture");

    let json = fx.run_json(&["list", "--json"]);
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    let entry = &arr[0];
    assert_eq!(entry["installName"], "demo");
    assert_eq!(
        entry["repo"],
        format!("{}:{}", repo_url, remote.skill_path())
    );
    assert_eq!(entry["skillPath"], "demo");
    assert_eq!(entry["description"], "fixture");
}
