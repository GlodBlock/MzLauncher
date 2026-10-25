use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mzlauncher_core::{paths::AppPaths, server::Server, CORE_VERSION};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut data_dir = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--data-dir" {
            let Some(path) = args.next() else {
                usage();
            };
            data_dir = Some(PathBuf::from(path));
        } else if arg == "--silent" {
            hide_console();
        } else if arg == "--version" {
            eprintln!("mzlauncher-core {CORE_VERSION}");
            return Ok(());
        } else {
            usage();
        }
    }

    let paths = AppPaths::discover(data_dir.clone())?;
    std::fs::create_dir_all(&paths.logs)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.logs.join("core.log"))?;
    let writer = Arc::new(Mutex::new(file));
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(move || LogWriter(writer.clone()))
        .init();

    Server::new(data_dir)?.run().await?;
    Ok(())
}

struct LogWriter(Arc<Mutex<std::fs::File>>);

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = std::io::stderr().write(buf)?;
        if let Ok(mut file) = self.0.lock() {
            let _ = file.write_all(buf);
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Ok(mut file) = self.0.lock() {
            let _ = file.flush();
        }
        std::io::stderr().flush()
    }
}

fn usage() -> ! {
    eprintln!("usage: mzlauncher-core [--data-dir <path>] [--silent] | [--version]");
    std::process::exit(2);
}

#[cfg(windows)]
fn hide_console() {
    use windows_sys::Win32::System::Console::GetConsoleWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    unsafe {
        let window = GetConsoleWindow();
        if !window.is_null() {
            ShowWindow(window, SW_HIDE);
        }
    }
}

#[cfg(not(windows))]
fn hide_console() {}
