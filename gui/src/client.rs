use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

pub enum CoreMessage {
    Response {
        id: String,
        result: Option<Value>,
        error: Option<Value>,
    },
    Event(Value),
    Disconnected(String, String),
}

pub struct CoreConnection {
    writer: Arc<Mutex<ChildStdin>>,
    counter: Arc<AtomicU64>,
    pub receiver: Receiver<CoreMessage>,
    pub core_path: PathBuf,
    child: Child,
}

impl CoreConnection {
    pub fn spawn() -> Result<Self, (String, String)> {
        let core_path =
            find_core().ok_or_else(|| ("error.core_not_found".to_owned(), String::new()))?;
        let mut command = Command::new(&core_path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .map_err(|error| ("error.core_spawn".to_owned(), error.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ("error.core_stdin".to_owned(), String::new()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ("error.core_stdout".to_owned(), String::new()))?;
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _ = sender.send(CoreMessage::Disconnected(
                            "error.core_exited".into(),
                            String::new(),
                        ));
                        break;
                    }
                    Ok(_) => {
                        let message = match serde_json::from_str::<Value>(line.trim()) {
                            Ok(value) => value,
                            Err(_) => continue,
                        };
                        if let Some(event) = message.get("event") {
                            if sender.send(CoreMessage::Event(event.clone())).is_err() {
                                break;
                            }
                        } else if let Some(id) = message.get("id").and_then(Value::as_str) {
                            let response = CoreMessage::Response {
                                id: id.to_owned(),
                                result: message.get("result").cloned(),
                                error: message.get("error").cloned(),
                            };
                            if sender.send(response).is_err() {
                                break;
                            }
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(CoreMessage::Disconnected(
                            "error.core_read".into(),
                            error.to_string(),
                        ));
                        break;
                    }
                }
            }
        });
        Ok(Self {
            writer: Arc::new(Mutex::new(stdin)),
            counter: Arc::new(AtomicU64::new(1)),
            receiver,
            core_path,
            child,
        })
    }

    pub fn request(&self, method: &str, params: Value) -> Result<String, String> {
        let id = format!("gui-{}", self.counter.fetch_add(1, Ordering::SeqCst));
        let message = json!({"id": id, "method": method, "params": params});
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| "core writer lock unavailable".to_owned())?;
        writeln!(writer, "{message}").map_err(|error| error.to_string())?;
        writer.flush().map_err(|error| error.to_string())?;
        Ok(id)
    }
}

impl Drop for CoreConnection {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn find_core() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MZLAUNCHER_CORE_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let executable = if cfg!(windows) {
        "mzlauncher-core.exe"
    } else {
        "mzlauncher-core"
    };
    if let Ok(current) = std::env::current_exe() {
        if let Some(directory) = current.parent() {
            let candidate = directory.join(executable);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(executable);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
