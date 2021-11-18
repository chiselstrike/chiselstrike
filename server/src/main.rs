// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

#[macro_use]
extern crate log;

use anyhow::Result;
use chisel_server::server;
use enclose::enclose;
use nix::unistd::execv;
use server::DoRepeat;
use std::env;
use std::ffi::CString;
use structopt::StructOpt;

#[tokio::main]
async fn main() -> Result<()> {
    let mut executors = vec![];

    pretty_env_logger::init();

    let args: Vec<CString> = env::args().map(|x| CString::new(x).unwrap()).collect();
    let (tasks, shared) = server::run_shared_state(server::Opt::from_args()).await?;
    let exe = env::current_exe()?.into_os_string().into_string().unwrap();

    for id in 0..shared.executor_threads() {
        debug!("Starting executor {}", id);
        executors.push(std::thread::spawn(enclose! { (shared) move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    server::run_on_new_localset(shared).await
                }).unwrap();
        }}));
    }

    for ex in executors.drain(..) {
        ex.join().unwrap();
    }

    if let DoRepeat::Yes = tasks.join().await? {
        info!("Restarting");
        execv(&CString::new(exe).unwrap(), &args).unwrap();
    }
    Ok(())
}
