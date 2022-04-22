// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::cmd::apply::{apply, AllowTypeDeletion, TypeChecking};
use crate::project::read_manifest;
use crate::server::{start_server, wait};
use crate::DEFAULT_API_VERSION;
use anyhow::Result;
use futures::channel::mpsc::channel;
use futures::{FutureExt, SinkExt, StreamExt};
use notify::{event::ModifyKind, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::panic;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use tokio::task::JoinHandle;

pub(crate) async fn cmd_dev(server_url: String, type_check: bool) -> Result<()> {
    let type_check = type_check.into();
    let manifest = read_manifest()?;
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_hook(info);
        nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
    }));
    let (mut signal_tx, mut signal_rx) = channel(1);
    let sig_task: JoinHandle<Result<()>> = tokio::task::spawn(async move {
        let _ = futures::select! {
            _ = sigterm.recv().fuse() => { },
            _ = sigint.recv().fuse() => { },
            _ = sighup.recv().fuse() => { },
        };
        signal_tx.send(()).await?;
        Ok(())
    });
    let mut server = start_server()?;
    wait(server_url.clone()).await?;
    apply_from_dev(server_url.clone(), type_check).await;
    let (mut watcher_tx, mut watcher_rx) = channel(1);
    let mut apply_watcher = RecommendedWatcher::new(move |res: Result<Event, notify::Error>| {
        futures::executor::block_on(async {
            watcher_tx.send(res).await.unwrap();
        });
    })?;
    let watcher_config = notify::Config::OngoingEvents(Some(Duration::from_millis(100)));
    apply_watcher.configure(watcher_config)?;
    for models_dir in &manifest.models {
        let models_dir = Path::new(models_dir);
        apply_watcher.watch(models_dir, RecursiveMode::Recursive)?;
    }
    for endpoints_dir in &manifest.endpoints {
        let endpoints_dir = Path::new(endpoints_dir);
        apply_watcher.watch(endpoints_dir, RecursiveMode::Recursive)?;
    }
    for policies_dir in &manifest.policies {
        let policies_dir = Path::new(policies_dir);
        apply_watcher.watch(policies_dir, RecursiveMode::Recursive)?;
    }
    loop {
        futures::select! {
            _ = signal_rx.next().fuse() => {
                break;
            }
            res = watcher_rx.next().fuse() => {
                let res = res.unwrap();
                match res {
                    Ok(Event {
                        kind: EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Name(_)),
                        paths,
                        ..
                    }) => {
                        let paths: HashSet<PathBuf> = HashSet::from_iter(
                            paths
                                .into_iter()
                                .filter(|path| !crate::project::ignore_path(path.to_str().unwrap())),
                        );
                        let paths: HashSet<PathBuf> = HashSet::from_iter(paths.into_iter());
                        if !paths.is_empty() {
                            apply_from_dev(server_url.clone(), type_check).await;
                        }
                    }
                    Ok(_) => { /* ignore */ }
                    Err(e) => println!("watch error: {:?}", e),
                }
            }
        }
    }
    server.kill()?;
    server.wait()?;
    sig_task.await??;

    Ok(())
}

async fn apply_from_dev(server_url: String, type_check: TypeChecking) {
    if let Err(e) = apply(
        server_url,
        DEFAULT_API_VERSION,
        AllowTypeDeletion::No,
        type_check,
    )
    .await
    {
        eprintln!("{:?}", e)
    }
}
