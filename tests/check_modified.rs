#[path = "support/mod.rs"]
mod support;

use serde_json::Value as Json;
use std::fs;
use support::CliFixture;

#[test]
fn check_reports_modified_after_local_edit() {
    let fix = CliFixture::new();
    let remote = fix.create_remote("repo-check-mod", "skill", "sfile");
    fix.install_from_remote(&remote, "sfile");

    let first: Json = fix.run_json(["check", "--json"].as_ref());
    assert_eq!(first[0]["state"].as_str().unwrap(), "ok");

    fs::write(fix.skill_dir("sfile").join("file.txt"), "v1 local edit\n").unwrap();

    let second: Json = fix.run_json(["check", "--json"].as_ref());
    assert_eq!(second[0]["state"].as_str().unwrap(), "modified");
}
