use anyhow::{ensure, Result};
use reqwest::{Response, Url};

// Drop the extension (.d.ts/.ts/.js) from a path
pub fn without_extension(path: &str) -> &str {
    for suffix in [".d.ts", ".ts", ".js"] {
        if let Some(s) = path.strip_suffix(suffix) {
            return s;
        }
    }
    path
}

// Simple wrapper over request::get that errors if the response status
// is not success.
pub async fn get_ok(url: Url) -> Result<Response> {
    let res = reqwest::get(url).await?;
    ensure!(res.status().is_success(), "HTTP request failed");
    Ok(res)
}
