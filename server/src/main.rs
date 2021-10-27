// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

#[macro_use]
extern crate log;

use anyhow::Result;
use chisel_server::server;
use nix::unistd::execv;
use server::DoRepeat;
use std::env;
use std::ffi::CString;
use structopt::StructOpt;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<CString> = env::args().map(|x| CString::new(x).unwrap()).collect();
    pretty_env_logger::init();
    if let DoRepeat::Yes = server::run_on_new_localset(server::Opt::from_args()).await? {
        let exe = env::current_exe()?.into_os_string().into_string().unwrap();
        info!("Restarting");
        execv(&CString::new(exe).unwrap(), &args).unwrap();
    }
    Ok(())
}
