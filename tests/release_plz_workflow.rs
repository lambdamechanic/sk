use std::path::PathBuf;

fn workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github")
        .join("workflows")
        .join("release-plz.yml")
}

#[test]
fn release_plz_workflow_exists_and_runs_release_plz() {
    let path = workflow_path();
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("expected {:?} to exist: {err}", path));

    assert!(
        contents.contains("release-plz/action@"),
        "release-plz action missing in {:?}",
        path
    );
    assert!(
        contents.contains("command: release"),
        "release-plz release command missing in {:?}",
        path
    );
    assert!(
        contents.contains("command: release-pr"),
        "release-plz release-pr command missing in {:?}",
        path
    );
}
