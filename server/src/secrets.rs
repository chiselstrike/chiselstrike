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

type RsaPayload = String;

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
    /// The V0 of the secrets is comprised of an RSA encrypted map of secrets.
    V0 { secret: RsaPayload },
    /// The V1 of the secrets is comprised of an AES encrypted map of secrets.
    V1 {
        #[serde(flatten)]
        inner: AesPayload,
    },
    /// The V2 of the secrets encrypts each secret independantly with AES. The map `secret` is an
    /// association of the secret name and the encrypted AES payload.
    V2 { secret: HashMap<String, AesPayload> },
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
        Payload::V1 { inner } => extract_v1(private_key, &inner)?,
        Payload::V2 { secret } => extract_v2(private_key, &secret)?,
    };

    Ok(secrets)
}

fn extract_v2(
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

fn extract_v1(private_key: &RsaPrivateKey, payload: &AesPayload) -> Result<JsonObject> {
    let s = decrypt_aes(private_key, payload)?;
    let secrets = serde_json::from_str(&s)?;

    Ok(secrets)
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

fn extract_v0(private_key: &RsaPrivateKey, ciphertext: &str) -> Result<JsonObject> {
    let bytes = base64::decode(ciphertext)?;
    let padding = PaddingScheme::new_pkcs1v15_encrypt();
    let dec_data = private_key
        .decrypt_blinded(&mut rand::rngs::OsRng, padding, &bytes)
        .map_err(|x| anyhow!("Failed to decrypt RSA payload: {:?}", x))?;
    let secrets = serde_json::from_slice(&dec_data)?;

    Ok(secrets)
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

    const PAYLOAD_V2: &str = "eyJ2ZXJzaW9uIjoiVjIiLCJzZWNyZXQiOnsibXlzZWNyZXQyIjp7InNlY3JldCI6ImkvVFlaR2l6SlE3TjdRUGV1ZDR5MjNpTENlTHU1KzRKRzNRVSIsImtleSI6Ik04Qkt2a3puT1k4VmMvMWs1aVpXbldzZHB6RXBHM1lYMk1PTHJBRHNrc20yMkFBRlB0SlVnTlBzMkZCSjl4WDBGQlFBOG9ZUlBlc2NWbXRhQXhoRHNkeCtRSU9kS21NM2lJNzBlY3dSeVlJMWFpVU84cnBuS0tBdkRXNkZJTXFPZ2dHbzdLZm01bmd0c055Y3QyR3ROeXUrQ20rUG1FSy9rVTltdnhUNVI0MUhycGJBaC96MXBaOWlzU2hrN3Ird1JFcnZmV2kvcEFXelY1WDFNK2NMZ3d3Nm9KTno0b1FnTjZ2OW9mOWdVNGluZis3OWJtWXovVG91MHV5VHhVemI3Q1ErUXV5NmM1bDd1QjJzbEhBQmVBVjB0aTJWU0tNK24yMndqNHcySHlXWWlUNXBwTWYybDI5OUFPRWVjVWRQK2RvTFdrOU5pTE5Gd1ZWTmRWaDM5M1c2akxudlFpSGV5aldjOHF6U3lyMzdwckNGejhFNFhKVzZITkNSdWQ3d2RyVmV3MmZpRG53WExtb3NyWlJiSFBvL1BuVlY4dGhXMzF4VG41L3RtMGdQd3YwZUhxdjQvdFhCMk5MOWF0WlhRTlVWS25nVkxJOGtJODAzT2ptd1hmN2lyQm5WVUlKZnNCbEllVEhkZEtmc2VGTVZNcnc5dWNuMkd0bUNZdTZVIiwibm9uY2UiOiJycE9OekxyZU00cGEvdkkwemJWZWpqUjdvT2I4aGZrVndrK2Fzd2ZIbVVVNWtjeTJkSkk4Rnc2R1MxY2orVXR0OTN0MkhsbXhCK2NHRW1GL1J0TzFYdENsRVY1OTE5cUx2UUhHdGd1ODVIcGJCZXN2UnBnYVZwTFVjQlJDYTRnRHMyeEhvYW1QU1l6cE05UldrK25mS1BtamxPVmFxanh2bGhwcVV2R0hBZEwzWGhJdXcybnZFRFl5eGNIWXd1UDFMQldsMlE3VndxOERQK1RFa01QUmRmTnNnOGJ0ZktmNmtJbmhoNkFxdVBDMGs2SEZsc0tadTBySU44Nk4wczREaXZBbmRmNTJ4aHFjUW9tMzdpVTJXNDlUcXVlWXp3NkhRallCVXNRdjdsaE5Ud2lqcGFoZFo0UFkzOVp5aXdianVFRldvdklRRTZ0NFk1eWd4S3cxOEVyQlNtUlcyUjRrNFpvSVZwQWNSQWROU2VxRlV5VUlBWEtWTDFRaUJlSGRIYjh3VTRjeTA5eHhCc0pwWUd0WTRUenNqTnU3TUVzVXlKd0JoRzVVWXg1SDVXazV1TU5HUHNnY1FNMHJmNnFoditwNXBUaTN5Q2tBV04rc0VkQ3VNcWVZMkxKMlZtclA0eFNPTWE2eXo4N3NteXQ4Zk5ldzhVOXRkOWR0MDUzTyIsImhhc2giOiJZd0V3ZHoxYThQTnRQNUFBOGxqNVB3MWtTWGFsT1hQQlFPaVh1d3FMSFNFPSIsImhhc2hTYWx0IjoiWDc3aitpb3ZIcXVmdUlHSyJ9LCJteXNlY3JldDEiOnsic2VjcmV0IjoiMFBZeXVxa2MyV2J5c3hlL09tNGtWOXNxdFpFRTJiLzh2aUJWIiwia2V5IjoiTUxOK1M3aDBJNGR5MnR6L0hQMk92MkcyTkV2N1o1T283MkNOTllpV05LWFJpMHN5anM2YUYzY3RPb3RnN1pRYjBvRXFkbzFkdllZZXd1bWhkR3hta2ppQ0dSWFpCc0JVRG1FV1ZoVitaaHJIZ0ZaV1JUbVptRmg5ZmhNOXdxWGd2cm5NaU0vaVlVbThQTWVCNHhwRHA1ZWJjRlBIcTRjUWZQSVVMbk5oTThVbFVwVjRmbjB4L21hSXIzOUJXS2JITU5yZlk2bnhndmdmcGgrWDlaWHl5SGtxR2ZPRzZJUUI2OTFEcVUzQysrbEdHbnRNcjhJSUxRZ000SVlHT1ZRWWpFWmduTGNLZTBkd1hNc2Z4UHJ5bkh5TEYyRGp5Q2hhQlF5UUtINTU0TEU5ZG1Ya0xybnltZmJMVlFEcVlZUXhucnhoZkdwS3NSaEsrTFNMTE8xZ0I0NEZaejllMkRtMTcrT21EQVROWlU5elplaXFyVStDd2d0eVpuTjFjRzZYOVRITmlxdTc5K1l1Tm93VWFUZTJlajJUTy9rNjBQYzZ4dmZyS2ZaRXVZUVNLR1J4VjIrOEtna1ZCdC9nQlpPYWVENnd3cXAzaUlvRE1ienM4Rzc4UHZXWGdjQ3N6YzJLVitIQmRaTDFiUE9UeGh1SlBIWkkwSnUwd0dXRU1zK3EiLCJub25jZSI6Ilh1ZWdHT1RVaSs1aG9vRzVFKzIrWXJHd1RNeThWSjlxRHVVZWJmazFNVXFGUXNaZVlVTE1VREtQTW9jcENRVWdXYXNZOHNkZ0UvZjBKQ3JFcTVVL0t1cHVQaFhaNklIa2F5NGpGRmt0aXBFcFptemZyd3VPYkNLZ09rVWVJTDhNUGMxNWdzM0VQL216WVEzSXdBSUpYL3MzZDJSaDN1RGNpKytXVzFmNS9rbjRZeCswNTI4UDEzcm4xWFRFcGs1NVpaSUhVQkRhVnovWWJXeHNFUlNoYnhvSzdxNGwxaEt3R05jSlE3WDc5T2poTXBMbDREbFpHbkhsb0FIS1ZkajdON0pPR1ZZRGdGbGdqSFQ3eWthVVpqQzNoUHhLc1NRQlc4S1hSY1hxZ01GK1FTc2wyWkkrcUMxYTBFZ0FoUmwrbFJvTFA3MGwwNkg4R1NKcXlQRUh0UXNoKytzU01rNnRGd3hIeGhoenBsd2pRK0dnM2FnbW9iVHlVNnNETEJxMks0RVpvMVZsMnJRY25FSTlEQ01zT1JDN1pMR2M2cWNycDJJM3UyT05janRwOW5VL0RMMGNvbUZwZlNneDR0VjhTK25ORnNVMDJRZnFodVhldStqVFkvbkhJZWlHOG9udU5UZXJ1S0Rsb055OTl3NTJmOXBNWGJYQld5bnhRSnFCIiwiaGFzaCI6ImE5K21KS3ZFTE1KOWdTMmZ6Y1VDbkpKN1hZLzZ1MTZsYXBjQ2V4c2NTQ2s9IiwiaGFzaFNhbHQiOiJKVDh2b1R4QnNqVkt0MS91In19fQ==";

    const PAYLOAD_V1: &str = "eyJ2ZXJzaW9uIjoiVjEiLCJzZWNyZXQiOiJSTVE1bHNNM3p4dHBjWjFhYmVpaThqdmpQSXNCemlNdFYzalgzRXRhRkRlSGFoemZwMmpZMW41RVdYcUs4aTBHdmQvQ2JYNko1ZlVQVTFYRkNXd2RTeDNraXpGNCIsIm5vbmNlIjoidng2M2NrbkJaMCs4SEVFNnIrQWdzQlpjdlU4V1RPVU1PSzM3akJwem9VamRseFlBZ2c5TVorbUZ1V0pyZEQrWjJpUFc0T1VhblhVSy9zVENNVEZLNlo3RHVCQTdobzN2MG9GcmVpMml5WTRFMFF4L1A0WCtvYmVUM3gwVmdCNCs4bW5rd0lZRzlUVVdjMWZjWDMvRzg0ZWFXZkkwZ0orYzRHQUh5czEyQ3hUVWxVb096SFk3SEI2TExobyt6bXVGMldkTHU4WEk4WG55c1dxMGd2WWVPb1JJSElNY1V4YUgyVHhaZEV2ci9qcGFnY2MvY3RJZThtVXd5eU9MMy9XcmpqU1E4VHdrUjJvdGZHTVgyZU5hdFlYRG1LTG54SVFyNlRITlpETGxNZUVzMDZ4UlJlamRlNkZUR3I0ckJ0NzZGbTJjMDlFZkhFVTBQOTlPWEIzUmViTDVDZmRFdldyd0hjSVhzWWEyNjdYZkRsc1JvK2toZzFpUlk0UHZvdHcvNWV0ZDNlbjdNVkxHcnQ3dkJ5UFQyYUdmTzFJbkVSVTFWSHVIUE1VUDkrVFR2THAvS0g3Sk8yVEY3YndSTld3SHFlbVh0R1Y1ajRBWmZ2VkFVU0lPWFR6eVVzNXI4azRoa3FZVDJIbXdGN0svak9BUHQ5WGdEbHhJRlRaOHE1aWgiLCJrZXkiOiJGOWorbzBXYWxaOUNhVEFoSWtNZHFTaEg1K1NYL3BRV0JhN1JvV00ramFoaGlnUUZVWmNrRmxuTFVDek8yeHBMcXZ0YXBwZGhXWWFqMHIyay9GekU2S0JlVWtUZk5vSjAvQUNobW5SOUFUMTVmWDhRaVVRZWNuclY3RDhlS1BVVDZQL1JrOFcyNFBaMGNLN2NvbHNBZG9OUWJDa2x0aWFYcCswdmpIUlBHcWpVb09hOXM2dXpKclcrUE91NmI1WDIrYUorenVPN2pxVll3N0hFTVpHVEw4ZUl5VldaTjM3a2VyWTNJcHV1V3lsK2pRU0twQmtiMC9BaTZ4bGNsN2RqNUJDandUM3R0K2lBazIrUTRzdUh3d25OTStLQ2hQcnBldjN5bnlmWnhWUGJDN3p1dStiRE50NWlNMERWTUtNMzNjTEk3SHNiOWZHdmVjUHltN0p3ekxKekVvWHd6QXo5cmZoU1NNcWpvNXIyZUlkdEgzMnZhQ3F0bFdISnY2NGJpYm90YTlzb0twOHFmYk0wbUM2byt6dFR6dnd3RVdvdW1EMGVMNFg3TFVtOCs1M2NGMnV0VnZzWkwxaW8ya0dGamgxN0szditGckR3T3k3aFVCaDlmY1FCTGpoNHEyeUt4NlFiN0RuaVk5WkkrNXNkMUhPcTNsSFhBUS80M29OcCJ9";

    const PAYLOAD_V0: &str = "eyJ2ZXJzaW9uIjoiVjAiLCJzZWNyZXQiOiJHSkdaWkJsMzJodlZWV01SQ1EvdEthaHVUV1FVN1MwQVRkdGhYQ0JzU0pWVFAvOWFweG5OaXJnNGRPVVZBNHZPeldsUWdzQldkSzZYaUVvOThraUNWTWtoVDhTTGErRnNsRjc2S1lKS0lhZ3U2MVRrUzFUak8rNU5ZSUEydElZZnJ5NytJalBidWF5NHFKbVJVQzQyaTVMUk1LcHozd0JkYnZPWDNyaEROam5lUjBVYTV2N2NzVkRCNlNQOE5lK2pNSFI2YXVXS2Yyc2lEcStPamNwVHI5NUU4U04ySS8vWU02T3JBdjBpRm10c25UZ05zdHA5QXdyOSt0bzFhZThFRWRWOEZRSGtrSS9wV1AvTjhvTVhmaTdFRFd1M1Z6Y0FCV3cyTGZVV2hFbXZ6OExrN3ByVjhBT0pCUHRVdGxTbGlUNXEzdWphQ05DV0hRMXZkSHBOdThsOU1NVWFTMFpmZmRmTGRub2pBdm1xUkxtczNMRGdVNVU1bjlGMUs4WTRIcjFNRzUyVGpMK2MrUzBQVnlKcm9XVU81YkdoS01aenZqSkJHWUFlUXZ1S0pwQ1lUWU0vWjFOcUd3eDE5dTdQSUVmSmdMeGpEYzlrU011SnRnN1QzVUViOHJtU0dOcFp2VGNIZnlnSHV2SC9FaktBUy92UnFsWnRRcjFPK0ZSZSJ9";

    #[test]
    fn test_extract_secrets_v2() {
        let actual = extract_secrets(&PRIVATE_KEY, PAYLOAD_V2).unwrap();

        let expected = json!({
            "mysecret1": "testsecret1",
            "mysecret2": "testsecret2",
        });

        assert_eq!(expected.as_object().unwrap(), &actual);
    }

    #[test]
    fn test_extract_secrets_v1() {
        let actual = extract_secrets(&PRIVATE_KEY, PAYLOAD_V1).unwrap();

        let expected = json!({
            "mysecret1": "testsecret1",
            "mysecret2": "testsecret2",
        });

        assert_eq!(expected.as_object().unwrap(), &actual);
    }

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
