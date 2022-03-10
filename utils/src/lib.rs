use anyhow::{ensure, Result};
use reqwest::{Response, Url};

// Simple wrapper over request::get that errors if the response status
// is not success.
pub async fn get_ok(url: Url) -> Result<Response> {
    let res = reqwest::get(url).await?;
    ensure!(res.status().is_success(), "HTTP request failed");
    Ok(res)
}
