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
        json!({"clientVersion": "test", "minProtocol": 2, "maxProtocol": 2}),
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

#[test]
fn logs_request_and_job_error_details() {
    let temp = tempfile::tempdir().unwrap();
    let broken = temp.path().join("broken.mrpack");
    std::fs::write(&broken, b"not a zip archive").unwrap();
    let logs = temp.path().join("logs");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mzlauncher-core"))
        .arg("--data-dir")
        .arg(temp.path().join("data"))
        .arg("--log-dir")
        .arg(&logs)
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
        json!({"clientVersion": "test", "minProtocol": 2, "maxProtocol": 2}),
    );
    let analysis = client.call("parse", "modpack.analyze", json!({"path": broken}));
    assert_eq!(analysis["error"]["code"], "archive_error");
    let import = client.call("import", "modpack.import", json!({"path": broken}));
    let job_id = import["result"]["id"].as_str().unwrap().to_owned();
    let mut status = String::new();
    for _ in 0..100 {
        let job = client.call("job", "job.get", json!(job_id));
        status = job["result"]["status"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if status == "failed" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(status, "failed");
    client.call("stop", "system.shutdown", Value::Null);
    drop(client);
    assert!(child.wait().unwrap().success());

    let log = std::fs::read_to_string(logs.join("core.log")).unwrap();
    let request_line = log
        .lines()
        .find(|line| line.contains("request failed") && line.contains("request_id=parse"))
        .unwrap_or_else(|| panic!("request error was not logged: {log}"));
    assert!(
        request_line.contains("method=modpack.analyze"),
        "{request_line}"
    );
    assert!(
        request_line.contains("code=\"archive_error\""),
        "{request_line}"
    );
    assert!(request_line.contains("archive error:"), "{request_line}");
    let job_line = log
        .lines()
        .find(|line| line.contains("job failed") && line.contains(&format!("job_id={job_id}")))
        .unwrap_or_else(|| panic!("job error was not logged: {log}"));
    assert!(job_line.contains("kind=Import"), "{job_line}");
    assert!(job_line.contains("target=None"), "{job_line}");
    assert!(job_line.contains("stage=running"), "{job_line}");
    assert!(job_line.contains("code=\"archive_error\""), "{job_line}");
    assert!(job_line.contains("archive error:"), "{job_line}");
}

#[test]
fn searches_remote_modpacks_and_validates_installation() {
    use std::io::Read;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            let body = if request.starts_with("GET /v1/modpacks/public/modpack/popular/plays/21 ") {
                json!({"status":"success","packs":[125]})
            } else if request.starts_with("GET /v1/modpacks/public/modpack/125 ") {
                json!({
                    "id":125,
                    "name":"FTB Evolution",
                    "synopsis":"Explore a new world.",
                    "authors":[{"name":"FTB Team"}],
                    "art":[{"type":"square","url":"https://cdn.example.com/pack.png"}],
                    "installs":1000,
                    "versions":[{
                        "id":499,
                        "name":"1.0.0",
                        "updated":10,
                        "private":false,
                        "targets":[
                            {"type":"game","name":"minecraft","version":"1.21.1"},
                            {"type":"modloader","name":"neoforge","version":"21.1.72"}
                        ]
                    }, {
                        "id":500,
                        "name":"1.1.0",
                        "type":"release",
                        "updated":20,
                        "private":false,
                        "targets":[
                            {"type":"game","name":"minecraft","version":"1.21.1"},
                            {"type":"modloader","name":"neoforge","version":"21.1.72"}
                        ]
                    }]
                })
            } else {
                panic!("unexpected request: {request}");
            };
            let body = body.to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let paths = mzlauncher_core::paths::AppPaths::new(data.clone());
    paths.initialize().unwrap();
    let mut config = mzlauncher_core::config::AppConfig::default();
    config.endpoints.ftb_api = format!("http://{address}/v1/modpacks");
    std::fs::write(
        paths.config_file(),
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mzlauncher-core"))
        .arg("--data-dir")
        .arg(data)
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
    assert!(handshake["result"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("modpack_browser")));
    let search = client.call(
        "search",
        "modpack.search",
        json!({"provider":"ftb","query":"","gameVersion":null,"loader":null}),
    );
    assert!(search["error"].is_null(), "search failed: {search}");
    assert_eq!(search["result"][0]["name"], "FTB Evolution");
    assert_eq!(search["result"][0]["gameVersions"], json!(["1.21.1"]));
    assert_eq!(search["result"][0]["loaders"], json!(["neo_forge"]));
    let versions = client.call(
        "versions",
        "modpack.versions",
        json!({"provider":"ftb","projectId":"125","gameVersion":null,"loader":null}),
    );
    assert!(versions["error"].is_null(), "versions failed: {versions}");
    assert_eq!(versions["result"][0]["fileId"], "500");
    assert_eq!(versions["result"][0]["versionName"], "1.1.0");
    assert_eq!(versions["result"][1]["fileId"], "499");
    let invalid = client.call(
        "invalid",
        "modpack.install",
        json!({"provider":"ftb","projectId":"../125","projectName":"FTB Evolution"}),
    );
    assert_eq!(invalid["error"]["code"], "invalid_input");
    client.call("stop", "system.shutdown", Value::Null);
    drop(client);
    assert!(child.wait().unwrap().success());
    server.join().unwrap();
}

#[test]
fn searches_modrinth_by_downloads_with_provider_offsets() {
    use std::io::Read;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for expected_offset in [0, 20, 40] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("GET /v2/search?"), "{request}");
            assert!(request.contains("limit=21"), "{request}");
            assert!(request.contains("index=downloads"), "{request}");
            assert!(
                request.contains(&format!("offset={expected_offset}")),
                "{request}"
            );
            let count = if expected_offset == 40 { 9 } else { 21 };
            let hits = (0..count)
                .map(|index| {
                    let project_index = expected_offset + index;
                    json!({
                        "project_id": project_index.to_string(),
                        "title": format!("Matching Pack {project_index}"),
                        "description": "Description",
                        "author": "Author",
                        "icon_url": null,
                        "slug": format!("matching-{project_index}"),
                        "downloads": 5000 - project_index,
                        "versions": ["1.21.1"],
                        "categories": ["fabric"]
                    })
                })
                .collect::<Vec<_>>();
            let body = json!({"hits": hits}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let paths = mzlauncher_core::paths::AppPaths::new(data.clone());
    paths.initialize().unwrap();
    let mut config = mzlauncher_core::config::AppConfig::default();
    config.endpoints.modrinth_api = format!("http://{address}/v2");
    std::fs::write(
        paths.config_file(),
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mzlauncher-core"))
        .arg("--data-dir")
        .arg(data)
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
        json!({"clientVersion": "test", "minProtocol": 2, "maxProtocol": 2}),
    );
    for (id, offset, expected_len, expected_name) in [
        ("first", 0, 21, "Matching Pack 0"),
        ("second", 20, 21, "Matching Pack 20"),
        ("last", 40, 9, "Matching Pack 40"),
    ] {
        let response = client.call(
            id,
            "modpack.search",
            json!({
                "provider": "modrinth",
                "query": "matching pack",
                "gameVersion": null,
                "loader": null,
                "offset": offset
            }),
        );
        assert!(response["error"].is_null(), "search failed: {response}");
        assert_eq!(response["result"].as_array().unwrap().len(), expected_len);
        assert_eq!(response["result"][0]["name"], expected_name);
    }
    client.call("stop", "system.shutdown", Value::Null);
    drop(client);
    assert!(child.wait().unwrap().success());
    server.join().unwrap();
}

#[test]
fn fetches_curseforge_modpack_changelog() {
    use std::io::Read;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for file_id in [500, 501] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(
                request.starts_with(&format!("GET /v1/mods/125/files/{file_id}/changelog ")),
                "{request}"
            );
            assert!(request
                .to_ascii_lowercase()
                .contains("x-api-key: test-key\r\n"));
            let data = if file_id == 500 {
                "<p>Added features &amp; fixes</p><ul><li>First</li><li>Second</li></ul>"
            } else {
                "  "
            };
            let body = json!({"data": data}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let paths = mzlauncher_core::paths::AppPaths::new(data.clone());
    paths.initialize().unwrap();
    let mut config = mzlauncher_core::config::AppConfig::default();
    config.endpoints.curseforge_api = format!("http://{address}/v1");
    std::fs::write(
        paths.config_file(),
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mzlauncher-core"))
        .arg("--data-dir")
        .arg(data)
        .env("MZLAUNCHER_CURSEFORGE_API_KEY", "test-key")
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
        json!({"clientVersion": "test", "minProtocol": 2, "maxProtocol": 2}),
    );
    let response = client.call(
        "changelog",
        "modpack.changelog",
        json!({"provider":"curse_forge","projectId":"125","fileId":"500"}),
    );
    assert!(response["error"].is_null(), "changelog failed: {response}");
    assert_eq!(
        response["result"],
        "Added features & fixes\n\nFirst\nSecond"
    );
    let empty = client.call(
        "empty",
        "modpack.changelog",
        json!({"provider":"curse_forge","projectId":"125","fileId":"501"}),
    );
    assert_eq!(empty["result"], "");
    let unsupported = client.call(
        "unsupported",
        "modpack.changelog",
        json!({"provider":"modrinth","projectId":"pack","fileId":"version"}),
    );
    assert_eq!(unsupported["result"], "");
    let invalid = client.call(
        "invalid",
        "modpack.changelog",
        json!({"provider":"curse_forge","projectId":"../125","fileId":"500"}),
    );
    assert_eq!(invalid["error"]["code"], "invalid_input");
    client.call("stop", "system.shutdown", Value::Null);
    drop(client);
    assert!(child.wait().unwrap().success());
    server.join().unwrap();
}

#[test]
fn downloads_and_caches_modpack_icons() {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use std::io::Read;

    let png = STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server_png = png.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let read = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..read]);
        assert!(request.starts_with("GET /icon.png "), "{request}");
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            server_png.len()
        )
        .unwrap();
        stream.write_all(&server_png).unwrap();
    });

    let temp = tempfile::tempdir().unwrap();
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
        json!({"clientVersion": "test", "minProtocol": 2, "maxProtocol": 2}),
    );
    let url = format!("http://{address}/icon.png");
    for id in ["first", "cached"] {
        let response = client.call(id, "modpack.icon", json!({"url": url}));
        assert!(response["error"].is_null(), "icon failed: {response}");
        assert_eq!(response["result"].as_str().unwrap(), STANDARD.encode(&png));
    }
    client.call("stop", "system.shutdown", Value::Null);
    drop(client);
    assert!(child.wait().unwrap().success());
    server.join().unwrap();
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
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).unwrap();
            assert!(!line.is_empty(), "core closed stdout");
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["event"].is_object() {
                continue;
            }
            assert_eq!(response["id"], id);
            return response;
        }
    }
}
