// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

#[macro_use]
extern crate log;

use anyhow::Result;
use chisel_server::server;
use env_logger::Env;
use log::LevelFilter;
use nix::unistd::execv;
use server::DoRepeat;
use std::env;
use std::ffi::CString;
use std::io::Write;
use structopt::StructOpt;

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

    let args: Vec<CString> = env::args().map(|x| CString::new(x).unwrap()).collect();
    let exe = env::current_exe()?.into_os_string().into_string().unwrap();

    if let DoRepeat::Yes = server::run_all(server::Opt::from_args()).await? {
        info!("Restarting");
        execv(&CString::new(exe).unwrap(), &args).unwrap();
    }
    Ok(())
}
