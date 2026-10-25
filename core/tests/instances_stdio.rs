use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

#[test]
fn instance_crud_works_through_the_real_protocol() {
    let temp = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mzlauncher-core"))
        .arg("--data-dir")
        .arg(temp.path())
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
    let created = client.call(
        "create",
        "instance.create",
        json!({
            "name": "测试实例",
            "gameVersion": "1.21.1",
            "loader": "vanilla",
            "minMemoryMb": 512,
            "maxMemoryMb": 4096
        }),
    );
    let id = created["result"]["id"].as_str().unwrap().to_owned();
    let copied = client.call("copy", "instance.copy", json!({"id": id, "name": "copy"}));
    assert_ne!(copied["result"]["id"], id);
    let listed = client.call("list", "instance.list", Value::Null);
    assert_eq!(listed["result"].as_array().unwrap().len(), 2);
    let validated = client.call("validate", "instance.validate", json!({"id": id}));
    assert_eq!(validated["result"]["valid"], false);
    client.call("delete", "instance.delete", json!({"id": id}));
    let listed = client.call("list-after-delete", "instance.list", Value::Null);
    assert_eq!(listed["result"].as_array().unwrap().len(), 1);
    client.call("stop", "system.shutdown", Value::Null);
    drop(client);
    assert!(child.wait().unwrap().success());
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
        assert!(response["error"].is_null(), "request failed: {response}");
        response
    }
}
