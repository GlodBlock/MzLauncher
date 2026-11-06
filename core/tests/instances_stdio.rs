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
    let handshake = client.call(
        "hello",
        "system.handshake",
        json!({"clientVersion": "test", "minProtocol": 2, "maxProtocol": 2}),
    );
    let capabilities = handshake["result"]["capabilities"].as_array().unwrap();
    assert!(capabilities.contains(&json!("lwjgl_override")));
    assert!(capabilities.contains(&json!("modpack_update")));
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
    assert_eq!(created["result"]["totalPlayTimeMs"], 0);
    let id = created["result"]["id"].as_str().unwrap().to_owned();
    let mut update = created["result"].clone();
    update["lwjglVersion"] = json!("3.3.3");
    let updated = client.call("update", "instance.update", update);
    assert_eq!(updated["result"]["lwjglVersion"], "3.3.3");
    let mods_directory = temp.path().join("instances").join(&id).join("mods");
    std::fs::create_dir_all(&mods_directory).unwrap();
    let jar = std::fs::File::create(mods_directory.join("example.jar")).unwrap();
    let mut writer = zip::ZipWriter::new(jar);
    writer
        .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer
        .write_all(br#"{"name":"Example Mod","version":"1.2.3","icon":"icon.png"}"#)
        .unwrap();
    writer
        .start_file("icon.png", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"png").unwrap();
    writer.finish().unwrap();
    let mods = client.call("mods", "mod.list", json!({"instanceId": id}));
    assert_eq!(mods["result"]["total"], 1);
    let installed = &mods["result"]["items"][0];
    assert_eq!(installed["file"]["projectName"], "Example Mod");
    assert_eq!(installed["file"]["versionName"], "1.2.3");
    let icon = client.call(
        "mod-icon",
        "mod.icon",
        json!({"id": installed["id"].as_str().unwrap()}),
    );
    assert_eq!(icon["result"], "cG5n");
    for index in 0..10 {
        std::fs::write(
            mods_directory.join(format!("manual-{index}.jar")),
            b"manual",
        )
        .unwrap();
    }
    let first_page = client.call(
        "mods-first-page",
        "mod.list",
        json!({"instanceId": id, "offset": 0, "limit": 10}),
    );
    assert_eq!(first_page["result"]["total"], 11);
    assert_eq!(first_page["result"]["items"].as_array().unwrap().len(), 10);
    let second_page = client.call(
        "mods-second-page",
        "mod.list",
        json!({"instanceId": id, "offset": 10, "limit": 10}),
    );
    assert_eq!(second_page["result"]["items"].as_array().unwrap().len(), 1);
    let modpack = client.call("modpack", "modpack.get", json!({"id": id}));
    assert!(modpack["result"].is_null());
    let copied = client.call("copy", "instance.copy", json!({"id": id, "name": "copy"}));
    assert_ne!(copied["result"]["id"], id);
    assert_eq!(copied["result"]["totalPlayTimeMs"], 0);
    let copied_id = copied["result"]["id"].as_str().unwrap().to_owned();
    client.call(
        "reorder",
        "instance.reorder",
        json!({"ids": [&id, &copied_id]}),
    );
    let listed = client.call("list", "instance.list", Value::Null);
    let items = listed["result"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["id"], id);
    assert_eq!(items[1]["id"], copied_id);
    let current = client.call("get-before-update", "instance.get", json!({"id": &id}));
    client.call(
        "update-after-reorder",
        "instance.update",
        current["result"].clone(),
    );
    let listed = client.call("list-after-update", "instance.list", Value::Null);
    let items = listed["result"].as_array().unwrap();
    assert_eq!(items[0]["id"], id);
    assert_eq!(items[1]["id"], copied_id);
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
