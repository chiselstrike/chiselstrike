// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::JsonObject;
use aes_gcm::aead::{Aead, NewAead};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::Context;
use anyhow::{anyhow, Result};
use deno_core::url;
use deno_core::url::Url;
use rsa::{PaddingScheme, RsaPrivateKey};
use serde::Deserialize;
use sha2::Sha256;
use std::collections::HashMap;
use std::str;

/// Represents an AES encrypted payload.
///
/// The secret is encrypted using a AES symmetric key K, and a nonce N. K and N and then encrypted
/// using the instance public RSA key, and added to the payload (key, and nonce fields).
///
/// The secret can be decrypted by first decrypting the key and the nonce using the instance
/// private RSA key, and then using the decrypted K and N to decrypt secret.
#[derive(Deserialize, Debug)]
struct AesPayload {
    #[serde(with = "serde_base64")]
    secret: Vec<u8>,
    #[serde(with = "serde_base64")]
    nonce: Vec<u8>,
    #[serde(with = "serde_base64")]
    key: Vec<u8>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "version")]
enum Payload {
    /// The V0 of the secrets encrypts each secret independantly with AES. The map `secret` is an
    /// association of the secret name and the encrypted AES payload.
    V0 { secret: HashMap<String, AesPayload> },
}

mod serde_base64 {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let base64 = String::deserialize(d)?;
        base64::decode(base64.as_bytes()).map_err(serde::de::Error::custom)
    }
}

async fn read_url(url: &Url) -> Result<String> {
    match url.scheme() {
        "file" => match tokio::fs::read_to_string(url.path()).await {
            Ok(data) => Ok(data),
            Err(x) => {
                if x.kind() == std::io::ErrorKind::NotFound {
                    Ok("{}".to_string())
                } else {
                    Err(x)
                }
            }
        }
        .map_err(|x| anyhow!("reading file {}: {:?}", url.as_str(), x)),
        _ => {
            let req = utils::get_ok(url.clone())
                .await
                .map_err(|x| anyhow!("reading URL {}: {:?}", url.as_str(), x))?;
            req.text()
                .await
                .map_err(|x| anyhow!("reading URL {}: {:?}", url.as_str(), x))
        }
    }
}

fn get_pkcs1_private_key(pem: &str) -> Result<Option<RsaPrivateKey>> {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    let mut key = RsaPrivateKey::from_pkcs1_pem(pem)?;
    key.precompute()?;
    Ok(Some(key))
}

fn get_pkcs8_private_key(pem: &str) -> Result<Option<RsaPrivateKey>> {
    use rsa::pkcs8::DecodePrivateKey;
    let mut key = RsaPrivateKey::from_pkcs8_pem(pem)?;
    key.precompute()?;
    Ok(Some(key))
}

pub(crate) async fn get_private_key() -> Result<Option<RsaPrivateKey>> {
    let url = match std::env::var("CHISEL_SECRET_KEY_LOCATION") {
        Err(_) => return Ok(None),
        Ok(x) => match Url::parse(&x) {
            Ok(o) => Ok(o),
            Err(url::ParseError::RelativeUrlWithoutBase) => {
                let cwd = std::env::current_dir()?;
                let path = cwd.join(x);
                Url::from_file_path(path).map_err(|_| anyhow!("Can't convert path to url"))
            }
            Err(err) => {
                anyhow::bail!("Invalid url: {:?}", err);
            }
        },
    }?;

    let pem = read_url(&url).await?;

    parse_rsa_key(&pem).with_context(|| format!("Could not read private key at {url}"))
}

fn parse_rsa_key(pem: &str) -> Result<Option<RsaPrivateKey>> {
    let pkcs1 = get_pkcs1_private_key(pem);
    if pkcs1.is_ok() {
        return pkcs1;
    }

    get_pkcs8_private_key(pem)
}

pub(crate) async fn get_secrets() -> Result<JsonObject> {
    let secret_location = match std::env::var("CHISEL_SECRET_LOCATION") {
        Ok(s) => Url::parse(&s)?,
        Err(_) => {
            let cwd = std::env::current_dir()?;
            Url::from_file_path(&cwd.join(".env")).unwrap()
        }
    };
    let private_key = get_private_key().await?;
    let data = read_url(&secret_location).await?;
    let secrets = match private_key {
        None => serde_json::from_str(&data)?,
        Some(private_key) => extract_secrets(&private_key, &data)?,
    };

    Ok(secrets)
}

fn extract_secrets(private_key: &RsaPrivateKey, payload: &str) -> Result<JsonObject> {
    let decoded = decode_base64(payload)?;
    let payload: Payload = serde_json::from_slice(&decoded)?;
    let secrets = match payload {
        Payload::V0 { secret } => extract_v0(private_key, &secret)?,
    };

    Ok(secrets)
}

fn extract_v0(
    private_key: &RsaPrivateKey,
    secrets: &HashMap<String, AesPayload>,
) -> Result<JsonObject> {
    let mut out = JsonObject::with_capacity(secrets.len());

    for (name, secret) in secrets {
        let decrypted = decrypt_aes(private_key, secret)?;
        out.insert(name.to_string(), decrypted.into());
    }

    Ok(out)
}

fn decrypt_aes(private_key: &RsaPrivateKey, payload: &AesPayload) -> Result<String> {
    let key = rsa_oaep_decrypt(private_key, &payload.key)
        .map_err(|x| anyhow!("Failed to decrypt AES key: {:?}", x))?;
    let nonce = rsa_oaep_decrypt(private_key, &payload.nonce)
        .map_err(|x| anyhow!("Failed to decrypt AES nonce: {:?}", x))?;

    let key = aes_gcm::Key::from_slice(&key);
    let cipher = Aes256Gcm::new(key);

    let message = cipher
        .decrypt(Nonce::from_slice(&nonce), payload.secret.as_ref())
        .map_err(|x| anyhow!("Failed to decrypt secrets: {:?}", x))?;

    let message = String::from_utf8(message)
        .map_err(|x| anyhow!("Failed to read decrypted secrets as utf8: {:?}", x))?;

    Ok(message)
}

fn rsa_oaep_decrypt(private_key: &RsaPrivateKey, data: &[u8]) -> Result<Vec<u8>> {
    let padding = PaddingScheme::new_oaep::<Sha256>();
    let dec_data = private_key
        .decrypt_blinded(&mut rand::rngs::OsRng, padding, data)
        .map_err(|x| anyhow!("Failed to decrypt: {:?}", x))?;
    Ok(dec_data)
}

fn decode_base64(data: &str) -> Result<Vec<u8>> {
    let mut data = data.trim_end().trim().as_bytes().to_vec();
    if data.last() == Some(&0) {
        data.pop();
    }
    let data = base64::decode(&data)?;
    Ok(data)
}
