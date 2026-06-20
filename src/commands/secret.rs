//! Secret / API-key encryption IPC. The proprietary build uses the
//! OS keychain (macOS Keychain, Windows Credential Manager,
////! Linux libsecret) to wrap an AES-GCM key; the clean-room
//! build derives the wrapping key from a passphrase the user
//! supplies at install time. We ship a placeholder here that
//! accepts the input, hashes it, and returns a stable token —
//! the wire format is identical, so the frontend's
//! `encrypt_secret` / `decrypt_secret` calls don't need to
//! change. A real production deployment should swap this for
//! `keyring` + `aes-gcm`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::command;

use crate::commands::ipc;
use crate::error::{CoreResult, Error};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptInput {
    pub plaintext: String,
    pub passphrase: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptOutput {
    pub ciphertext: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecryptInput {
    pub ciphertext: String,
    pub passphrase: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecryptOutput {
    pub plaintext: String,
}

#[command]
pub fn encrypt_secret(input: EncryptInput) -> Result<EncryptOutput, String> {
    ipc(encrypt(input))
}

#[command]
pub fn decrypt_secret(input: DecryptInput) -> Result<DecryptOutput, String> {
    ipc(decrypt(input))
}

fn derive_key(passphrase: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(passphrase.as_bytes());
    hasher.update(b"echobird-core-v1");
    let out = hasher.finalize();
    let mut k = [0u8; 32];
    k.copy_from_slice(&out);
    k
}

fn encrypt(input: EncryptInput) -> CoreResult<EncryptOutput> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use rand::RngCore;
    let key = derive_key(&input.passphrase);
    let cipher = Aes256Gcm::new(&key.into());
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), input.plaintext.as_bytes())
        .map_err(|e| Error::internal(format!("aes-gcm encrypt: {e}")))?;
    let mut blob = Vec::with_capacity(12 + ct.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    Ok(EncryptOutput {
        ciphertext: format!("enc:v1:{}", hex::encode(blob)),
    })
}

fn decrypt(input: DecryptInput) -> CoreResult<DecryptOutput> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    let blob = input
        .ciphertext
        .strip_prefix("enc:v1:")
        .ok_or_else(|| Error::validation("ciphertext missing enc:v1: prefix"))?;
    let bytes = hex::decode(blob).map_err(|e| Error::validation(format!("hex: {e}")))?;
    if bytes.len() < 12 {
        return Err(Error::validation("ciphertext too short"));
    }
    let (nonce, ct) = bytes.split_at(12);
    let key = derive_key(&input.passphrase);
    let cipher = Aes256Gcm::new(&key.into());
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| Error::unauthorized("decryption failed (wrong passphrase?)"))?;
    Ok(DecryptOutput {
        plaintext: String::from_utf8(pt)
            .map_err(|e| Error::validation(format!("non-utf8 plaintext: {e}")))?,
    })
}
