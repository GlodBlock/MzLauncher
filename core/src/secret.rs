use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/builtin_secrets.rs"));

pub fn builtin_microsoft_client_id() -> Option<String> {
    decrypt(MICROSOFT_CLIENT_ID)
}

pub fn builtin_curseforge_api_key() -> Option<String> {
    decrypt(CURSEFORGE_API_KEY)
}

fn decrypt(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    String::from_utf8(crypt(data, BUILTIN_KEY)).ok()
}

fn crypt(data: &[u8], key: &[u8]) -> Vec<u8> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_restores_plaintext() {
        let key = [7_u8; 32];
        for text in ["", "short", "a longer value that spans multiple sha256 blocks"] {
            let encrypted = crypt(text.as_bytes(), &key);
            if text.is_empty() {
                assert!(encrypted.is_empty());
            } else {
                assert_ne!(encrypted, text.as_bytes());
                assert_eq!(String::from_utf8(crypt(&encrypted, &key)).unwrap(), text);
            }
        }
    }
}
