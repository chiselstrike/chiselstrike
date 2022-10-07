// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::cmd::apply::{apply, AllowTypeDeletion, TypeChecking};
use crate::project::read_manifest;
use crate::server::wait;
use crate::DEFAULT_API_VERSION;
use anyhow::Result;
use deno_core::futures;
use endpoint_tsc::tsc_compile;
use futures::channel::mpsc::channel;
use futures::{SinkExt, StreamExt};
use notify::{
    event::ModifyKind, Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::collections::HashSet;
use std::env;
use std::panic;
use std::path::PathBuf;
use std::time::Duration;
use tokio::task::JoinHandle;
use tsc_compile::deno_core;

pub(crate) async fn cmd_dev(
    server_url: String,
    type_check: bool,
) -> Result<JoinHandle<Result<()>>> {
    let type_check = type_check.into();
    let cwd = env::current_dir()?;
    let manifest = read_manifest(&cwd)?;
    let (signal_tx, mut signal_rx) = utils::make_signal_channel();
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let sig_task: JoinHandle<Result<()>> = tokio::task::spawn(async move {
        tokio::select! {
            _ = sigterm.recv() => { },
            _ = sigint.recv() => { },
            _ = sighup.recv() => { },
        };
        signal_tx.send(()).await?;
        Ok(())
    });
    wait(server_url.clone()).await?;
    apply_from_dev(server_url.clone(), type_check).await;
    let (mut watcher_tx, mut watcher_rx) = channel(1);
    let config = Config::default()
        .with_poll_interval(Duration::from_millis(100))
        .with_compare_contents(true);
    let mut apply_watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            futures::executor::block_on(async {
                watcher_tx.send(res).await.unwrap();
            });
        },
        config,
    )?;

    let mut tracked = HashSet::new();
    let cwd = std::env::current_dir()?;

    tracked.extend(manifest.models.iter().map(|d| cwd.join(d)));
    tracked.extend(manifest.policies.iter().map(|d| cwd.join(d)));
    tracked.extend(manifest.routes.iter().map(|d| cwd.join(d)));
    tracked.extend(
        manifest
            .events
            .unwrap_or_default()
            .iter()
            .map(|d| cwd.join(d)),
    );
    apply_watcher.watch(&cwd, RecursiveMode::Recursive)?;

    loop {
        tokio::select! {
            _ = signal_rx.next() => {
                break;
            }
            res = watcher_rx.next() => {
                let res = res.unwrap();
                match res {
                    Ok(Event {
                        kind: EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Name(_)),
                        paths,
                        ..
                    }) => {
                        let is_tracked = |x: &PathBuf| {
                            for p in tracked.iter() {
                                if x.starts_with(p) {
                                    return !crate::project::ignore_path(x.to_str().unwrap());
                                }
                            }
                            false
                        };

                        if paths.iter().any(is_tracked) {
                            apply_from_dev(server_url.clone(), type_check).await;
                        }
                    }
                    Ok(_) => { /* ignore */ }
                    Err(e) => eprintln!("watch error: {:?}", e),
                }
            }
        }
    }
    Ok(sig_task)
}

async fn apply_from_dev(server_url: String, type_check: TypeChecking) {
    if let Err(e) = apply(
        server_url,
        DEFAULT_API_VERSION.to_string(),
        AllowTypeDeletion::No,
        type_check,
    )
    .await
    {
        eprintln!("{:?}", e)
    }
}
