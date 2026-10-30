use std::{
    io::Write,
    process::{Command, Stdio},
};

use cinnabar_cxb::fixtures;
use server_experience::{bundle::offline, crypto};

#[test]
fn offline_metadata_binds_the_complete_archive() {
    let bundle = fixtures::generate().unwrap().bundle;
    let metadata = offline::verify(&bundle).unwrap();
    assert_eq!(metadata.sha256, crypto::digest(&bundle));
    assert_eq!(metadata.bytes, bundle.len() as u64);
    let mut changed = bundle.clone();
    changed[0] ^= 1;
    assert!(offline::verify(&changed).is_err());
    assert!(offline::verify(b"not an archive").is_err());
}

#[test]
fn cli_verifies_the_supplied_stdin_snapshot() {
    let bytes = fixtures::generate().unwrap().bundle;
    let mut child = Command::new(env!("CARGO_BIN_EXE_cinnabar-cxb"))
        .args(["verify", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(metadata["sha256"], crypto::digest(&bytes));
    assert_eq!(metadata["bytes"], bytes.len() as u64);
    assert_eq!(metadata["manifest"]["id"], "benergistics");
}
