// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use chisel_server as server;
use env_logger::Env;
use log::LevelFilter;
use std::io::Write;
use std::path::PathBuf;
use structopt::StructOpt;

fn find_default_config_path() -> Option<PathBuf> {
    let config_dir = dirs::config_dir()?.join("chiselstrike");
    let config_path = config_dir.join("config.toml");
    config_path.exists().then_some(config_path)
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format(|buf, record| {
            writeln!(
                buf,
                "[{}] {} - {}",
                buf.timestamp(),
                record.level(),
                record.args()
            )
        })
        .filter_module("sqlx::query", LevelFilter::Warn)
        .init();

    let opt = {
        let default_path = find_default_config_path();
        let opt = match default_path {
            Some(ref path) => server::Opt::from_file(path).await?,
            None => server::Opt::from_args(),
        };

        match opt.config {
            Some(ref path) => server::Opt::from_file(path).await?,
            None => opt,
        }
    };

    if opt.show_config {
        let config = serde_json::to_string(&opt)?;
        println!("{config}");
        return Ok(());
    }

    server::run(opt).await
}
