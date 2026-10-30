# MzLauncher

MzLauncher is a Minecraft: Java Edition launcher with a core / gui split architecture: core is a headless standalone process handling downloads, instance management, authentication and launching; gui talks to core through a stable protocol, and the two can be upgraded independently.

## Features

- Isolated instances: game versions, loaders, mods, configs and saves never leak between instances
- Game and loaders: Vanilla plus Fabric, Forge and NeoForge installation with version matching checks
- Mod management: search, install, update and dependency resolution from Modrinth and CurseForge
- Modpack installation: browse CurseForge, FTB and Modrinth packs, or import local CurseForge and Modrinth archives with path safety checks
- Microsoft account login: official OAuth flow, credentials stored in the OS secure store, passwords never touched
- Download management: concurrency limits, resume, checksum verification, retries and recovery after interruption
- Game launching: process management, live log capture and redaction of sensitive data

## Usage

### Build

Rust 1.85 or newer is required:

```bash
cargo build --release
```

The binaries are at `target/release/mzlauncher-gui` and `target/release/mzlauncher-core`.

### Run

Start `mzlauncher-gui`. It launches the core process automatically, finding it next to the gui executable, on `PATH`, or at the path in the `MZLAUNCHER_CORE_PATH` environment variable.

In the gui you can log in with a Microsoft account, create an instance, wait for installation to finish, and launch the game. The main navigation contains Instances, Accounts, and Settings; instance details include mod management, logs, and launch overrides.

core communicates with gui over stdin/stdout and is not used as a command line tool directly. Any program that can read and write stdio can also drive core through the protocol.

### Package

On Windows, run `powershell -File scripts/package.ps1` to build three zip packages in `dist/`: `core` (headless core only), `gui` (interface only) and `full` (gui + core; extract and double-click `mzlauncher-gui.exe` to run).

## License

This project is released under the [GNU Lesser General Public License v3.0](LICENSE).
