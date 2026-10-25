use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

struct Logger {
    file: std::fs::File,
    path: PathBuf,
}

static LOGGER: OnceLock<Mutex<Option<Logger>>> = OnceLock::new();

pub fn init() {
    let logger = LOGGER.get_or_init(|| Mutex::new(None));
    let mut slot = logger.lock().unwrap();
    if slot.is_some() {
        return;
    }
    let directory = if cfg!(windows) {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("logs")))
    } else {
        None
    };
    let directory = match directory {
        Some(directory) => directory,
        None => {
            let Some(dirs) = directories::ProjectDirs::from("com", "MzLauncher", "MzLauncher")
            else {
                return;
            };
            dirs.data_local_dir().join("gui").join("logs")
        }
    };
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let path = directory.join("gui.log");
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        *slot = Some(Logger { file, path });
    }
}

pub fn path() -> Option<PathBuf> {
    LOGGER
        .get()?
        .lock()
        .ok()?
        .as_ref()
        .map(|logger| logger.path.clone())
}

pub fn write(message: &str) {
    let Some(logger) = LOGGER.get() else { return };
    let Ok(mut slot) = logger.lock() else { return };
    let Some(logger) = slot.as_mut() else { return };
    use std::io::Write;
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let _ = writeln!(logger.file, "[{timestamp}] {message}");
    let _ = logger.file.flush();
}
