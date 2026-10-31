use std::path::Path;

use mzlauncher_core::config::DownloadConfig;
use mzlauncher_core::download::{DownloadSpec, Downloader};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn resumes_with_a_valid_partial_response() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        assert!(request.to_ascii_lowercase().contains("range: bytes=6-"));
        socket
            .write_all(
                b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 6-10/11\r\nETag: \"test\"\r\n\r\nworld",
            )
            .await
            .unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("file.bin");
    let url = format!("http://{address}/file");
    write_partial(&destination, &url, b"hello ").await;
    let downloader = Downloader::new(DownloadConfig::default()).unwrap();
    downloader
        .download(spec(&url, &destination, 11), None)
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(destination).await.unwrap(), b"hello world");
    server.await.unwrap();
}

#[tokio::test]
async fn restarts_when_server_ignores_range() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        assert!(request.to_ascii_lowercase().contains("range: bytes=6-"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\nhello world")
            .await
            .unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("file.bin");
    let url = format!("http://{address}/file");
    write_partial(&destination, &url, b"stale!").await;
    let downloader = Downloader::new(DownloadConfig::default()).unwrap();
    downloader
        .download(spec(&url, &destination, 11), None)
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(destination).await.unwrap(), b"hello world");
    server.await.unwrap();
}

#[tokio::test]
async fn restarts_when_resume_range_returns_not_found() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        assert!(request.to_ascii_lowercase().contains("range: bytes=6-"));
        socket
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        drop(socket);

        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        assert!(!request.to_ascii_lowercase().contains("range:"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\nhello world")
            .await
            .unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("file.bin");
    let url = format!("http://{address}/file");
    write_partial(&destination, &url, b"stale!").await;
    let downloader = Downloader::new(DownloadConfig::default()).unwrap();
    downloader
        .download(spec(&url, &destination, 11), None)
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(destination).await.unwrap(), b"hello world");
    server.await.unwrap();
}

#[tokio::test]
async fn never_commits_checksum_mismatch() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nwrong")
            .await
            .unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("file.bin");
    let url = format!("http://{address}/file");
    let downloader = Downloader::new(DownloadConfig::default()).unwrap();
    let error = downloader
        .download(spec(&url, &destination, 5), None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "checksum_mismatch");
    assert!(!destination.exists());
    server.await.unwrap();
}

fn spec(url: &str, destination: &Path, size: u64) -> DownloadSpec {
    DownloadSpec {
        url: url.into(),
        destination: destination.into(),
        size: Some(size),
        sha1: Some("2aae6c35c94fcfb415dbe95f408b9ce91ee846ed".into()),
        sha256: None,
        provider: "test".into(),
    }
}

async fn write_partial(destination: &Path, url: &str, contents: &[u8]) {
    let partial = destination.with_file_name(".file.bin.part");
    let metadata = partial.with_extension("json");
    tokio::fs::write(&partial, contents).await.unwrap();
    tokio::fs::write(
        metadata,
        serde_json::to_vec(&serde_json::json!({
            "url": url,
            "size": 11,
            "sha1": "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed",
            "sha256": null,
            "etag": null,
            "lastModified": null
        }))
        .unwrap(),
    )
    .await
    .unwrap();
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = socket.read(&mut buffer).await.unwrap();
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}
