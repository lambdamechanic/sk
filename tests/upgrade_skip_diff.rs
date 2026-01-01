use std::fs;

#[path = "support/mod.rs"]
mod support;

use support::CliFixture;

#[test]
fn upgrade_shows_inline_diff_for_dirty_skills() {
    let fix = CliFixture::new();
    let remote = fix.create_remote("skip-diff", "skill", "sfile");
    fix.install_from_remote(&remote, "sfile");

    // Local edits plus a remote update so the skip path is taken.
    fs::write(fix.skill_dir("sfile").join("file.txt"), "local change\n").unwrap();
    remote.overwrite_file("file.txt", "upstream change\n", "v2");

    let out = fix.sk_cmd().args(["upgrade", "--all"]).output().unwrap();
    assert!(
        out.status.success(),
        "upgrade should still succeed: {out:?}"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("local edits plus upstream update"),
        "skip warning should mention upstream update\n{stdout}"
    );
    assert!(
        stdout.contains("Diff local vs upstream"),
        "diff header should be printed\n{stdout}"
    );
    assert!(
        stdout.contains("local/"),
        "diff output should include local prefix\n{stdout}"
    );
    assert!(
        stdout.contains("upstream change"),
        "diff output should show upstream changes\n{stdout}"
    );
}
