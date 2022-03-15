// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::JsonObject;
use aes_gcm::aead::{Aead, NewAead};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::Context;
use anyhow::{anyhow, Result};
use rsa::{PaddingScheme, RsaPrivateKey};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::str;
use url::Url;

type RsaPayload = Vec<u8>;

#[derive(Serialize, Deserialize, Debug)]
struct AesPayload {
    #[serde(with = "serde_base64")]
    secret: Vec<u8>,
    #[serde(with = "serde_base64")]
    nonce: Vec<u8>,
    #[serde(with = "serde_base64")]
    key: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum Payload {
    V0(RsaPayload),
    V1(AesPayload),
}

mod serde_base64 {
    use serde::{Deserialize, Serialize};
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        let base64 = base64::encode(v);
        String::serialize(&base64, s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let base64 = String::deserialize(d)?;
        base64::decode(base64.as_bytes()).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Default)]
pub(crate) struct SecretManager {
    secrets: JsonObject,
}

impl SecretManager {
    pub(crate) fn update_secrets(&mut self, new_secrets: JsonObject) {
        self.secrets = new_secrets;
    }

    pub(crate) fn get_secret<S: AsRef<str>>(&self, key: S) -> Option<serde_json::Value> {
        self.secrets.get(key.as_ref()).cloned()
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
    use rsa::pkcs1::FromRsaPrivateKey;
    let mut key = RsaPrivateKey::from_pkcs1_pem(pem)?;
    key.precompute()?;
    Ok(Some(key))
}

fn get_pkcs8_private_key(pem: &str) -> Result<Option<RsaPrivateKey>> {
    use rsa::pkcs8::FromPrivateKey;
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
    let pkcs1 = get_pkcs1_private_key(&pem);
    if pkcs1.is_ok() {
        return pkcs1;
    }

    get_pkcs8_private_key(&pem).with_context(|| format!("Could not read private key at {}", url))
}

pub(crate) async fn get_secrets(
    secret_location: &Url,
    private_key: &Option<RsaPrivateKey>,
) -> Result<String> {
    let data = read_url(secret_location).await?;
    match private_key {
        None => Ok(data),
        Some(private_key) => decrypt(private_key, &data),
    }
}

fn decrypt(private_key: &RsaPrivateKey, payload: &str) -> Result<String> {
    let payload = parse(payload)?;
    let secrets = match payload {
        Payload::V0(rsa) => decrypt_v0(private_key, &rsa),
        Payload::V1(aes) => decrypt_v1(private_key, &aes),
    }?;
    Ok(secrets)
}

fn parse(data: &str) -> Result<Payload> {
    let data = decode_base64(data)?;
    if let Ok(encoded) = str::from_utf8(&data) {
        if let Ok(payload) = serde_json::from_str::<Payload>(encoded) {
            return Ok(payload);
        }
    }
    Ok(Payload::V0(data))
}

fn decrypt_v1(private_key: &RsaPrivateKey, payload: &AesPayload) -> Result<String> {
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

fn decrypt_v0(private_key: &RsaPrivateKey, ciphertext: &[u8]) -> Result<String> {
    let padding = PaddingScheme::new_pkcs1v15_encrypt();
    let dec_data = private_key
        .decrypt_blinded(&mut rand::rngs::OsRng, padding, ciphertext)
        .map_err(|x| anyhow!("Failed to decrypt RSA payload: {:?}", x))?;
    Ok(String::from_utf8_lossy(&dec_data).to_string())
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
    use rsa::{BigUint, PublicKey, RsaPublicKey};

    fn get_private_key() -> RsaPrivateKey {
        RsaPrivateKey::from_components(
            rsa::BigUint::parse_bytes(b"00d397b84d98a4c26138ed1b695a8106ead91d553bf06041b62d3fdc50a041e222b8f4529689c1b82c5e71554f5dd69fa2f4b6158cf0dbeb57811a0fc327e1f28e74fe74d3bc166c1eabdc1b8b57b934ca8be5b00b4f29975bcc99acaf415b59bb28a6782bb41a2c3c2976b3c18dbadef62f00c6bb226640095096c0cc60d22fe7ef987d75c6a81b10d96bf292028af110dc7cc1bbc43d22adab379a0cd5d8078cc780ff5cd6209dea34c922cf784f7717e428d75b5aec8ff30e5f0141510766e2e0ab8d473c84e8710b2b98227c3db095337ad3452f19e2b9bfbccdd8148abf6776fa552775e6e75956e45229ae5a9c46949bab1e622f0e48f56524a84ed3483b", 16).unwrap(),
            BigUint::parse_bytes(b"10001", 16).unwrap(),
            BigUint::parse_bytes(b"00c4e70c689162c94c660828191b52b4d8392115df486a9adbe831e458d73958320dc1b755456e93701e9702d76fb0b92f90e01d1fe248153281fe79aa9763a92fae69d8d7ecd144de29fa135bd14f9573e349e45031e3b76982f583003826c552e89a397c1a06bd2163488630d92e8c2bb643d7abef700da95d685c941489a46f54b5316f62b5d2c3a7f1bbd134cb37353a44683fdc9d95d36458de22f6c44057fe74a0a436c4308f73f4da42f35c47ac16a7138d483afc91e41dc3a1127382e0c0f5119b0221b4fc639d6b9c38177a6de9b526ebd88c38d7982c07f98a0efd877d508aae275b946915c02e2e1106d175d74ec6777f5e80d12c053d9c7be1e341", 16).unwrap(),
            vec![
                BigUint::parse_bytes(b"00f827bbf3a41877c7cc59aebf42ed4b29c32defcb8ed96863d5b090a05a8930dd624a21c9dcf9838568fdfa0df65b8462a5f2ac913d6c56f975532bd8e78fb07bd405ca99a484bcf59f019bbddcb3933f2bce706300b4f7b110120c5df9018159067c35da3061a56c8635a52b54273b31271b4311f0795df6021e6355e1a42e61",16).unwrap(),
                BigUint::parse_bytes(b"00da4817ce0089dd36f2ade6a3ff410c73ec34bf1b4f6bda38431bfede11cef1f7f6efa70e5f8063a3b1f6e17296ffb15feefa0912a0325b8d1fd65a559e717b5b961ec345072e0ec5203d03441d29af4d64054a04507410cf1da78e7b6119d909ec66e6ad625bf995b279a4b3c5be7d895cd7c5b9c4c497fde730916fcdb4e41b", 16).unwrap()
            ],
        )
    }

    fn encrypt_v0(public_key: &RsaPublicKey, data: &[u8]) -> Result<Vec<u8>> {
        let padding = PaddingScheme::new_pkcs1v15_encrypt();
        let ciphertext = public_key
            .encrypt(&mut rand::rngs::OsRng, padding, data)
            .map_err(|x| anyhow!("Failed to encrypt: {:?}", x))?;
        Ok(ciphertext)
    }

    fn rsa_encrypt(public_key: &RsaPublicKey, data: &[u8]) -> Result<Vec<u8>> {
        let padding = PaddingScheme::new_oaep::<Sha256>();
        let ciphertext = public_key
            .encrypt(&mut rand::rngs::OsRng, padding, data)
            .map_err(|x| anyhow!("Failed to encrypt: {:?}", x))?;
        Ok(ciphertext)
    }

    fn encrypt_v1(public_key: &RsaPublicKey, data: &[u8]) -> Result<AesPayload> {
        let key = b"secret key that no one can know!";
        let key = aes_gcm::Key::from_slice(key);
        let cipher = Aes256Gcm::new(key);

        let nonce = b"such random!";
        let nonce = Nonce::from_slice(nonce);

        let ciphertext = cipher
            .encrypt(nonce, data.as_ref())
            .expect("encryption failure!");

        let key = rsa_encrypt(public_key, key.as_slice()).unwrap();
        let nonce = rsa_encrypt(public_key, nonce.as_slice()).unwrap();

        Ok(AesPayload {
            key,
            secret: ciphertext,
            nonce,
        })
    }

    fn serialize(payload: &Payload) -> String {
        match payload {
            Payload::V0(data) => base64::encode(data),
            Payload::V1(data) => base64::encode(&serde_json::to_vec(data).unwrap()),
        }
    }

    #[test]
    fn test_v0_decryption() {
        let private_key = get_private_key();
        let public_key = private_key.to_public_key();
        let data = "a bunch of arbitrary data to be encrypted";

        let ciphertext = encrypt_v0(&public_key, data.as_bytes()).unwrap();
        let ciphertext = serialize(&Payload::V0(ciphertext));

        let decrypted = decrypt(&private_key, &ciphertext).unwrap();
        assert_eq!(decrypted, data)
    }

    #[test]
    fn test_v1_decryption() {
        let private_key = get_private_key();
        let public_key = private_key.to_public_key();
        let data = "a bunch of arbitrary data to be encrypted";

        let ciphertext = encrypt_v1(&public_key, data.as_bytes()).unwrap();
        let ciphertext = serialize(&Payload::V1(ciphertext));

        let decrypted = decrypt(&private_key, &ciphertext).unwrap();
        assert_eq!(decrypted, data)
    }

    #[test]
    fn test_parse_v0() {
        let payload = "anything that is not a valid AesPayload serialized as JSON";
        let payload = base64::encode(payload);
        let payload = parse(&payload).unwrap();
        assert!(matches!(payload, Payload::V0(_)));
    }

    #[test]
    fn test_parse_v1() {
        let payload = r#"{"key": "Y2hpc2Vsc3RyaWtlIHJ1bGVz", "nonce": "c29tZSBrZXk=", "secret": "amFuIGlzIHNtYXJ0"}"#;
        let payload = base64::encode(payload);
        let payload = parse(&payload).unwrap();
        assert!(matches!(payload, Payload::V1(_)));
    }

    #[test]
    fn test_parse_error() {
        let payload = "not a valid base64.";
        assert!(parse(payload).is_err());
    }
}
