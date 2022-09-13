// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{ensure, Result};
use reqwest::{Response, Url};
use std::panic;

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

pub fn make_signal_channel() -> (async_channel::Sender<()>, async_channel::Receiver<()>) {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_hook(info);
        nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
    }));
    async_channel::bounded(1)
}
