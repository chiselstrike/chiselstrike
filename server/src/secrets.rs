// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::JsonObject;
use anyhow::{anyhow, Result};
use rsa::{PaddingScheme, RsaPrivateKey};
use url::Url;

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
            let req = reqwest::get(url.clone())
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

    get_pkcs8_private_key(&pem)
}

pub(crate) async fn get_secrets(
    secret_location: &Url,
    private_key: &Option<RsaPrivateKey>,
) -> Result<String> {
    let data = read_url(secret_location).await?;

    let data = match private_key {
        None => data,
        Some(private_key) => {
            let mut data = data.trim_end().trim().as_bytes().to_vec();
            if data.last() == Some(&0) {
                data.pop();
            }
            let base = base64::decode(&data)?;
            let padding = PaddingScheme::new_pkcs1v15_encrypt();
            let dec_data = private_key
                .decrypt_blinded(&mut rand::rngs::OsRng, padding, &base)
                .map_err(|x| anyhow!("Failed to decrypt: {:?}", x))?;
            String::from_utf8_lossy(&dec_data).to_string()
        }
    };
    Ok(data)
}
