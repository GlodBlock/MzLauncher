use std::env;
use std::fs;
use std::path::Path;

use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};

fn main() {
    let mut key = [0_u8; 32];
    OsRng.fill_bytes(&mut key);
    let mut generated = format!("pub static BUILTIN_KEY: &[u8] = &{:?};\n", key);
    for (constant, file) in [
        ("MICROSOFT_CLIENT_ID", "microsoft_client_id"),
        ("CURSEFORGE_API_KEY", "curseforge_api_key"),
    ] {
        let path = Path::new("../secret").join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        let value = fs::read_to_string(&path)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let data = value
            .map(|value| crypt(value.as_bytes(), &key))
            .unwrap_or_default();
        generated.push_str(&format!("pub static {constant}: &[u8] = &{:?};\n", data));
    }
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    fs::write(Path::new(&out_dir).join("builtin_secrets.rs"), generated)
        .expect("write generated secrets");
}

fn crypt(data: &[u8], key: &[u8; 32]) -> Vec<u8> {
    let mut output = Vec::with_capacity(data.len());
    for (index, chunk) in data.chunks(32).enumerate() {
        let mut hasher = Sha256::new();
        hasher.update(key);
        hasher.update((index as u64).to_le_bytes());
        let block = hasher.finalize();
        output.extend(chunk.iter().zip(block.iter()).map(|(a, b)| a ^ b));
    }
    output
}
