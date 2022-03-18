// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

#[macro_use]
extern crate log;

use anyhow::Result;
use chisel_server::server;
use enclose::enclose;
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
    let mut executors = vec![];

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
    let (tasks, shared, mut commands) = server::run_shared_state(server::Opt::from_args()).await?;
    let exe = env::current_exe()?.into_os_string().into_string().unwrap();

    for id in 0..shared.executor_threads() {
        debug!("Starting executor {}", id);
        let cmd = commands.pop().unwrap();
        executors.push(std::thread::spawn(enclose! { (shared) move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    server::run_on_new_localset(shared, cmd).await
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
