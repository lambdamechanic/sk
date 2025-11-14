use std::fs;

#[test]
fn digest_treats_crlf_and_lf_equally_for_text_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file.txt");
    fs::write(&path, "line1\r\nline2\r\n").unwrap();
    let crlf = sk::digest::digest_dir(dir.path()).unwrap();

    fs::write(&path, "line1\nline2\n").unwrap();
    let lf = sk::digest::digest_dir(dir.path()).unwrap();

    assert_eq!(crlf, lf, "digest should ignore CRLF vs LF differences");
}
