use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

#[test]
fn analyzes_modrinth_pack_and_rejects_unsafe_paths() {
    let temp = tempfile::tempdir().unwrap();
    let valid = temp.path().join("valid.mrpack");
    write_zip(&valid, &[
        (
            "modrinth.index.json",
            br#"{"formatVersion":1,"game":"1.21.1","versionId":"1.0.0","name":"Test Pack","dependencies":{"minecraft":"1.21.1","fabric-loader":"0.16.10"},"files":[]}"#.as_slice(),
        ),
        ("overrides/config/client.toml", b"enabled = true".as_slice()),
    ]);
    let curseforge = temp.path().join("curseforge.zip");
    write_zip(&curseforge, &[
        (
            "manifest.json",
            br#"{"manifestType":"minecraftModpack","manifestVersion":1,"name":"CF Pack","version":"1.0","author":"Author","minecraft":{"version":"1.21.1","modLoaders":[{"id":"fabric-0.16.10","primary":true}]},"files":[{"projectID":238222,"fileID":123456,"required":true}],"overrides":"overrides"}"#.as_slice(),
        ),
        ("overrides/options.txt", b"lang=zh_CN".as_slice()),
    ]);
    let malicious = temp.path().join("malicious.mrpack");
    write_zip(&malicious, &[
        ("../escape.jar", b"bad".as_slice()),
        ("modrinth.index.json", br#"{"formatVersion":1,"game":"1.21.1","name":"Bad","dependencies":{"minecraft":"1.21.1"},"files":[]}"#.as_slice()),
    ]);
    let duplicate = temp.path().join("duplicate.mrpack");
    write_zip(&duplicate, &[
        ("overrides/config/client.toml", b"one".as_slice()),
        ("overrides/CONFIG/client.toml", b"two".as_slice()),
        ("modrinth.index.json", br#"{"formatVersion":1,"game":"1.21.1","name":"Duplicate","dependencies":{"minecraft":"1.21.1"},"files":[]}"#.as_slice()),
    ]);

    let mut child = Command::new(env!("CARGO_BIN_EXE_mzlauncher-core"))
        .arg("--data-dir")
        .arg(temp.path().join("data"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut client = Client {
        stdin: child.stdin.take().unwrap(),
        stdout: BufReader::new(child.stdout.take().unwrap()),
    };
    client.call(
        "hello",
        "system.handshake",
        json!({"clientVersion": "test", "minProtocol": 1, "maxProtocol": 1}),
    );
    let analysis = client.call("analyze", "modpack.analyze", json!({"path": valid}));
    assert!(analysis["error"].is_null(), "analysis failed: {analysis}");
    assert_eq!(analysis["result"]["name"], "Test Pack");
    assert_eq!(analysis["result"]["loader"], "fabric");
    assert_eq!(
        analysis["result"]["overridePrefixes"],
        json!(["overrides/"])
    );
    let curseforge = client.call("curseforge", "modpack.analyze", json!({"path": curseforge}));
    assert!(
        curseforge["error"].is_null(),
        "CF analysis failed: {curseforge}"
    );
    assert_eq!(curseforge["result"]["name"], "CF Pack");
    assert_eq!(curseforge["result"]["modCount"], 1);
    assert_eq!(curseforge["result"]["author"], "Author");
    let malicious = client.call("malicious", "modpack.analyze", json!({"path": malicious}));
    assert_eq!(malicious["error"]["code"], "invalid_input");
    let duplicate = client.call("duplicate", "modpack.analyze", json!({"path": duplicate}));
    assert_eq!(duplicate["error"]["code"], "invalid_input");
    client.call("stop", "system.shutdown", Value::Null);
    drop(client);
    assert!(child.wait().unwrap().success());
    assert!(!temp.path().join("escape.jar").exists());
}

fn write_zip(path: &std::path::Path, entries: &[(&str, &[u8])]) {
    let file = std::fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    for (name, contents) in entries {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap();
}

struct Client {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Client {
    fn call(&mut self, id: &str, method: &str, params: Value) -> Value {
        writeln!(
            self.stdin,
            "{}",
            json!({"id": id, "method": method, "params": params})
        )
        .unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        assert!(!line.is_empty(), "core closed stdout");
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], id);
        response
    }
}
