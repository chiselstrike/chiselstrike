// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use chisel_server::server;

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();
    server::run_on_new_localset().await
}

