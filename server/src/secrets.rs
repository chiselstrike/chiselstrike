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

#[cfg(test)]
mod tests {

    use super::*;
    use once_cell::sync::Lazy;
    use serde_json::json;

    const PRIVATE_KEY_PEM: &str = r#"-----BEGIN RSA PRIVATE KEY-----
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
-----END RSA PRIVATE KEY-----"#;

    static PRIVATE_KEY: Lazy<RsaPrivateKey> =
        Lazy::new(|| parse_rsa_key(PRIVATE_KEY_PEM).unwrap().unwrap());

    const PAYLOAD_V0: &str = "eyJ2ZXJzaW9uIjoiVjAiLCJzZWNyZXQiOnsibXlzZWNyZXQxIjp7InNlY3JldCI6Im1VRmsrM0F6N2tXYjhSaDBRRFlMTW9NRmVjRW5EMmxYZTBwYSIsImtleSI6InRabGtHOVdYcHEvZjJhM1BUMndjZHNubng0a1REbG43ODlLMFVDZlIzMis3YysxMFcyWTNIQ21OL3N5Z1hML0tZSEs1dlJxQjQ4d1dFSzhRcWdqQTE2SmRGZlcyVGhGa25GdjJpUzZKVHRxMU8wUTVocTNqc2lVR2ZabnJwWVBXOXF2QjZtVVpBWmMyRC8yRXNpMkhQaDBtUzh3UWpwYnZzcDVlbmJHZU5tbWZZTHhjSEF6cmVPUzV5UkcwYXFYYWI4RVo5N0RsaXRCemV4Sk5SQkZ5UGNYcWg1MWFMOEViemFWVGtuUDlDNHhWUThmamt2UER3SWNrQzRjVlBsdEdHMHZDRlFJTk5VQ29vUzE0d291Qm5ianBST21jSWs3cy90K3hWRnZvZ0pNYzRvaW1NbTFzOUpUcnFURjFVN3hWRmFBTEF6U2FyRG42M0JYb1hUeXBPdGlSd2lMYndSSXFMeG5VZXJvZWpQZVVCQkkyeEtKZERDYzdwN1gra3I2L3RlajlxeXBNWW9YWE91UU45czBKclJmQUY1Z05HQVoxOW1qY1B6aDZNbFd4ZG5Ya2c1bHZpK0xYamxhbU9wK0lQT3FzUmgvaFZQc0xUZFRrbnNxS3NRc0dneDQzL1k2YzlBa1ZGM3J5U3pUaGJrUHVtclJUbGhHejEvSG9ZM20rIiwibm9uY2UiOiJkNnJwUEtyNEJ6Q0NtVWJkMExROHljcjN2WjhmVjI4THVSeUM5a1VvOEtMWlNUU1NXYU82c014STZiSWFnd1VsQ0lyQ3p1b0xqTEZKSENQWDRiZ1dPaEczYlhnSDBKWW02aGZuU0owNHpFbUxUZDFabWxHR3dlZmdyd056TEhMbjMvNkFjUVdUY01RMmtUUFhXTW4xQmRDSkQrUDQwSE1NTlR5MklrSXNyMDcxSVRDR3ovVXQ5ZVlVQXZTNGY1N3VjQTU2VlZPMVhzcmM3RVV1UFVqYXcza0JwaXZBSzVlYVVNZXA0cWNBU25Uek5ZdjJyejl0aGhlSGcyWFRkYko4K3ZEVWptMnNrRkIvTVFUdXYwcENGdmJmb0p6TW9Qa3VXOG5pWktqNHZsZnl4R3Q3VHNuV3c5U2NpViswZ3NkbTB1MFd0b1ZVamQwcTZaMlVnczRhWThiT0h6TFZOVXhQT09ySURpUElreXo3ZDZpQ1JBSWNnME50R3hKVisxbDdrN0d4cTJiNUhUdUN5ZlY0ZGk5bGsrU250dzJtb2Nsb3dmZ0lJbjBxV1BjYW9UUlVBcm1DTld2QTJPU0JaL1Z2b2VhelhYV2xzeXlZSHpINk8rSU51MFZ0K2J2QUpab1FudnBwdTBVT0hFbTBmeFdNZXFhdy9nQXJEd09RVkk5ViIsImhhc2giOiJjelcxUTI0cUpOS3lJeFA2cG81YlBGMnpEV1lSUWZoUnJma041Wk9FbmRFPSIsInNhbHQiOiJLNHBCdG9BeDdQeTBXNXlxIn0sIm15c2VjcmV0MiI6eyJzZWNyZXQiOiIzVGF5REVRaW9PSVRYSnVkblFiTHJBcHlFZ2tPOW1VdEZ3cVEiLCJrZXkiOiJIeFZTamJRcldpQkpmRDBMdjBkWXdnZmREZWNTM3k4QythQlNTMkhESTJmaUVQem8zRzdENGp4cjNOTXlVdTVGeGhjKzZwbUozL1Y0bTVubUQwYi9PZHEraWtia0JsVmZ3TksvM0p3N0E4dHgxZCs4cXBiRWw5ZWRGUjU5REkxZU5La0J6cENKeG1lUHVwMEZTY0J5dUNvdWNWS0lpd1luMStydnF4bG5jNk1CNXZqQnlEV2svaVFDb3haWGgvNmV2VDJGN0laY2pKbFFtNmVGWHgreUZPVVVYWm1PcXF1TFZTMkthY1RUYm1ydEljUEJHZi9NQ29wTERvdEFQakM5bmg3ZXMwRzhQM3NxbVdFL1NnV2M5U2Z3OUovSDVSTVR2a05RWUQwU3Y5TDlvS3VNTmUrYTNlUkdXbkZXMmZQYk5BWFo4S0gyclN2V1RFRXdQME44TEVPeC9hd0xGNEJEbCt4cDZ2WjU2ekYwVDdnM0wwNWhsTjZvZUplWG5nOWgySjJqbUl0bHIrSEpINW9mOVJnSndhNFVTbEYzWk5rWlhzRTFDSUxIL0RYdDVmRVRpOE1ST3ZMb0hHbEJ3Z3hEcnZ4bDNuOHhZaUQyWGJXMlMzS08xLzIvdkVYY21rYnZWNzFOQ3BKdjlpTW5xeDVkUHByd2RDZWFqT1VXeC9wZyIsIm5vbmNlIjoiVE02WDJxUmlvU2VOTmlLQXRheTFoQmg2TU5jN1VaZGRHWGV4S2Q4UEduWmFqWWJ0V1VZVlBsZ2RqNlJGWnBBaVNNUFgxYWU0WllMa0xLdkF0eFVnRjlFNVZTa3JDWGhnWkRYeC9rZUFzY0E2Q3BIbkF3cDF5REd4M1d6M0dTSUl5MlRiTll6MTM1YWlQa1pDNzh3dW9QRkV0NUU4SGR6ZnFqZ3NKR3oxbFdxWithc3RqcWwvdFVWREZVcHZMV1R4b1g2N2NEVXUxdFJqTEJONXppL2lndGdCaTAwSzdHVWRXWjljdHBsZkJESDBOOWNmOXVWamJxbml2UThKUjFwUE5ua0RPNVhyUUxEckxPRVI4YkJhZzYzUWJ6Q3FiMjA5K3RMU2F0SC80dDJDR0NpZS8rektmRlNOVlhEYUdKcTVobEpYODROMExtaURhVEFiR2t3bHJNNVNQczd2TGt5S1U3VnQwMkZCdnZYclpRS1BkRnY0Zjd1YWNjMStCOTRIWEN3bmxWcVk3MkttQzhEMHVNV0RxbEx3K3IwWXJXc0JWYXlYUWFFK3dRcTFkZ3lSS0VQTkwyWEU0b3NPOFdXdkNheG9ndGZ3eG9ZT0Y0WDFNM1lMbE85bVhWUndWcnJ3WHNRd1owT1p5ZXZSemhId3ZEZkVVa2s1eVBMVHN3WGciLCJoYXNoIjoiL3orVXQ1dmtGbk9PeEFGMEhkdHhzczRFbkU4ZHVLdHZsb3dCQXl3bk11UT0iLCJzYWx0IjoieEtacGo2eUQ0RFBTbFFXcCJ9fX0=";

    #[test]
    fn test_extract_secrets_v0() {
        let actual = extract_secrets(&PRIVATE_KEY, PAYLOAD_V0).unwrap();

        let expected = json!({
            "mysecret1": "testsecret1",
            "mysecret2": "testsecret2",
        });

        assert_eq!(expected.as_object().unwrap(), &actual);
    }
}
