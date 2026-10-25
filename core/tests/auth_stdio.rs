use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

#[test]
fn debug_account_is_controlled_by_the_compile_time_feature() {
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
    let handshake = client.call(
        "hello",
        "system.handshake",
        json!({"clientVersion": "test", "minProtocol": 1, "maxProtocol": 1}),
    );
    assert_eq!(handshake["result"]["debugAuth"], cfg!(feature = "debug"));
    let microsoft = client.call("microsoft", "account.begin_login", Value::Null);
    assert_eq!(microsoft["error"]["code"], "configuration_error");
    let debug = client.call(
        "debug",
        "account.debug_login",
        json!({"name": "Debug_Player"}),
    );
    if cfg!(feature = "debug") {
        assert!(debug["error"].is_null(), "debug login failed: {debug}");
        assert_eq!(debug["result"]["kind"], "debug");
        assert_eq!(debug["result"]["verified"], false);
        let id = debug["result"]["id"].as_str().unwrap();
        let selected = client.call("select", "account.select", json!({"id": id}));
        assert_eq!(selected["result"]["id"], id);
        let current = client.call("current", "account.current", Value::Null);
        assert_eq!(current["result"]["kind"], "debug");
    } else {
        assert_eq!(debug["error"]["code"], "forbidden");
    }
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
        response
    }
}
