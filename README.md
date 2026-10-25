# MzLauncher

MzLauncher is a Minecraft: Java Edition launcher with a core / gui split architecture: core is a headless standalone process handling downloads, instance management, authentication and launching; gui talks to core through a stable protocol, and the two can be upgraded independently.

## Features

- Isolated instances: game versions, loaders, mods, configs and saves never leak between instances
- Game and loaders: Vanilla plus Fabric, Forge and NeoForge installation with version matching checks
- Mod management: search, install, update and dependency resolution from Modrinth and CurseForge
- Modpack import: local CurseForge and Modrinth modpacks, with path safety checks before writing to disk
- Microsoft account login: official OAuth flow, credentials stored in the OS secure store, passwords never touched
- Download management: concurrency limits, resume, checksum verification, retries and recovery after interruption
- Game launching: process management, live log capture and redaction of sensitive data

## Usage

### Build

Rust 1.85 or newer is required:

```bash
cargo build --release
```

The binary is at `target/release/mzlauncher-core`.

### Run

core communicates with gui over stdin/stdout and is not used as a command line tool directly. Once gui starts and connects to the core process, all features are available. Any program that can read and write stdio can also drive core through the protocol.

## License

This project is released under the [GNU Lesser General Public License v3.0](LICENSE).
