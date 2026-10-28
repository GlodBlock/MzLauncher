use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::Value;

#[test]
fn core_process_performs_handshake_and_settings_roundtrip() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let logs = temp.path().join("logs");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mzlauncher-core"))
        .arg("--data-dir")
        .arg(&data)
        .arg("--log-dir")
        .arg(&logs)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    writeln!(
        stdin,
        "{{\"id\":\"hello\",\"method\":\"system.handshake\",\"params\":{{\"clientVersion\":\"test\",\"minProtocol\":2,\"maxProtocol\":2}}}}"
    )
    .unwrap();
    writeln!(
        stdin,
        "{{\"id\":\"settings\",\"method\":\"settings.get\",\"params\":null}}"
    )
    .unwrap();
    writeln!(
        stdin,
        "{{\"id\":\"diagnostics\",\"method\":\"system.diagnostics\",\"params\":null}}"
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
    assert_eq!(handshake["result"]["protocolVersion"], 2);
    assert_eq!(handshake["result"]["dataVersion"], 2);
    assert_eq!(handshake["result"]["debugAuth"], cfg!(feature = "debug"));
    let settings = read_response(&mut stdout, "settings");
    assert_eq!(settings["result"]["version"], 2);
    assert_eq!(settings["result"]["launch"]["minMemoryMb"], 512);
    assert_eq!(settings["result"]["launch"]["maxMemoryMb"], 4096);
    assert_eq!(
        settings["result"]["launch"]["jvmArgs"],
        serde_json::json!([])
    );
    let diagnostics = read_response(&mut stdout, "diagnostics");
    let extra = &diagnostics["result"]["extra"];
    assert_eq!(extra["dataDirectory"], data.to_string_lossy().as_ref());
    assert_eq!(
        extra["instanceDirectory"],
        data.join("instances").to_string_lossy().as_ref()
    );
    assert_eq!(extra["logDirectory"], logs.to_string_lossy().as_ref());
    let shutdown = read_response(&mut stdout, "stop");
    assert!(shutdown["error"].is_null());
    assert!(child.wait().unwrap().success());
    assert!(logs.join("core.log").is_file());
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
