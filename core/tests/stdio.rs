use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::Value;

#[test]
fn core_process_performs_handshake_and_settings_roundtrip() {
    let temp = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mzlauncher-core"))
        .arg("--data-dir")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    writeln!(
        stdin,
        "{{\"id\":\"hello\",\"method\":\"system.handshake\",\"params\":{{\"clientVersion\":\"test\",\"minProtocol\":1,\"maxProtocol\":1}}}}"
    )
    .unwrap();
    writeln!(
        stdin,
        "{{\"id\":\"settings\",\"method\":\"settings.get\",\"params\":null}}"
    )
    .unwrap();
    writeln!(
        stdin,
        "{{\"id\":\"stop\",\"method\":\"system.shutdown\",\"params\":null}}"
    )
    .unwrap();
    stdin.flush().unwrap();
    drop(stdin);

    let handshake = read_response(&mut stdout, "hello");
    assert_eq!(handshake["result"]["protocolVersion"], 1);
    assert_eq!(handshake["result"]["debugAuth"], cfg!(feature = "debug"));
    let settings = read_response(&mut stdout, "settings");
    assert_eq!(settings["result"]["version"], 1);
    let shutdown = read_response(&mut stdout, "stop");
    assert!(shutdown["error"].is_null());
    assert!(child.wait().unwrap().success());
}

fn read_response(stdout: &mut impl BufRead, expected_id: &str) -> Value {
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    assert!(!line.is_empty(), "core closed stdout unexpectedly");
    let value: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(value["id"], expected_id, "unexpected response: {value}");
    assert!(value["error"].is_null(), "request failed: {value}");
    value
}
