#![cfg(feature = "debug")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha1::{Digest, Sha1};

#[test]
fn launches_a_real_java_process_through_protocol() {
    let temp = tempfile::tempdir().unwrap();
    let classes = temp.path().join("classes");
    std::fs::create_dir_all(&classes).unwrap();
    let source = temp.path().join("TestMain.java");
    std::fs::write(
        &source,
        "public class TestMain { public static void main(String[] args) { System.out.println(\"TEST_MAIN_STARTED \" + String.join(\" \", args)); } }",
    )
    .unwrap();
    let status = Command::new("javac")
        .arg("-d")
        .arg(&classes)
        .arg(&source)
        .status()
        .unwrap();
    assert!(status.success());
    let jar = temp.path().join("client.jar");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&jar).unwrap());
    writer
        .start_file("TestMain.class", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer
        .write_all(&std::fs::read(classes.join("TestMain.class")).unwrap())
        .unwrap();
    writer.finish().unwrap();
    let jar_bytes = std::fs::read(&jar).unwrap();
    let jar_sha1 = hex::encode(Sha1::digest(&jar_bytes));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let base = format!("http://{address}");
    let details = json!({
        "id": "test",
        "type": "release",
        "mainClass": "TestMain",
        "assets": "test",
        "assetIndex": {"id": "test", "url": format!("{base}/assets.json")},
        "downloads": {"client": {"url": format!("{base}/client.jar"), "size": jar_bytes.len(), "sha1": jar_sha1}},
        "libraries": [],
        "arguments": {
            "jvm": ["-cp", "${classpath}"],
            "game": ["--username", "${auth_player_name}", "--accessToken", "${auth_access_token}"]
        },
        "javaVersion": {"component": "java-runtime-delta", "majorVersion": 17}
    });
    let manifest = json!({
        "versions": [{"id": "test", "type": "release", "url": format!("{base}/test.json")}]
    });
    let responses = vec![manifest.to_string(), details.to_string()];
    let server = std::thread::spawn(move || {
        for body in responses {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = [0_u8; 2048];
            let _ = socket.read(&mut buffer).unwrap();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });

    let data = temp.path().join("data");
    let version_dir = data.join("content/versions/test");
    std::fs::create_dir_all(&version_dir).unwrap();
    std::fs::copy(&jar, version_dir.join("test.jar")).unwrap();
    let mut config = mzlauncher_core::config::AppConfig::default();
    config.endpoints.version_manifest = format!("{base}/manifest.json");
    std::fs::create_dir_all(data.join("config")).unwrap();
    std::fs::write(
        data.join("config/settings.json"),
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mzlauncher-core"))
        .arg("--data-dir")
        .arg(&data)
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
    let account = client.call(
        "debug",
        "account.debug_login",
        json!({"name": "Debug_Player"}),
    )["result"]
        .clone();
    let java = client.call("java", "java.list", Value::Null)["result"][0].clone();
    let mut instance = client.call(
        "create",
        "instance.create",
        json!({
            "name": "launch-test",
            "gameVersion": "test",
            "loader": "vanilla",
            "accountId": account["id"],
            "javaPath": java["path"],
            "minMemoryMb": 256,
            "maxMemoryMb": 512
        }),
    )["result"]
        .clone();
    instance["installStatus"] = json!("installed");
    let connection = rusqlite::Connection::open(data.join("state/core.sqlite3")).unwrap();
    connection
        .execute(
            "UPDATE instances SET payload = ?1 WHERE id = ?2",
            rusqlite::params![
                serde_json::to_string(&instance).unwrap(),
                instance["id"].as_str().unwrap()
            ],
        )
        .unwrap();
    let preflight = client.call("preflight", "game.preflight", json!({"id": instance["id"]}));
    assert_eq!(preflight["result"]["valid"], true, "{preflight}");
    let launched = client.call("launch", "game.launch", json!({"id": instance["id"]}));
    assert!(launched["error"].is_null(), "launch failed: {launched}");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let status = client.call("status", "game.status", json!({"id": instance["id"]}));
        if status["result"]["status"] == "stopped" {
            assert_eq!(status["result"]["exitCode"], 0);
            break;
        }
        assert!(Instant::now() < deadline, "game did not stop: {status}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let logs = client.call(
        "logs",
        "game.logs",
        json!({"id": instance["id"], "tail": 50}),
    );
    let logs = logs["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(logs.contains("TEST_MAIN_STARTED"), "{logs}");
    assert!(logs.contains("Debug_Player"), "{logs}");
    client.call("shutdown", "system.shutdown", Value::Null);
    drop(client);
    assert!(child.wait().unwrap().success());
    server.join().unwrap();
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
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).unwrap();
            assert!(!line.is_empty(), "core closed stdout");
            let response: Value = serde_json::from_str(&line).unwrap();
            if response.get("id").is_none() {
                continue;
            }
            assert_eq!(response["id"], id);
            assert!(response["error"].is_null(), "request failed: {response}");
            return response;
        }
    }
}
