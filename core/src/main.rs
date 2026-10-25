use std::path::PathBuf;

use mzlauncher_core::{server::Server, CORE_VERSION};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

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

    Server::new(data_dir)?.run().await?;
    Ok(())
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
