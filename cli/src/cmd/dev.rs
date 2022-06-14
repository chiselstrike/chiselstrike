// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::cmd::apply::{apply, AllowTypeDeletion, TypeChecking};
use crate::project::read_manifest;
use crate::server::{start_server, wait};
use crate::DEFAULT_API_VERSION;
use anyhow::Result;
use deno_core::futures;
use endpoint_tsc::tsc_compile;
use futures::channel::mpsc::channel;
use futures::{SinkExt, StreamExt};
use notify::{event::ModifyKind, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;
use tsc_compile::deno_core;

pub(crate) async fn cmd_dev(server_url: String, type_check: bool) -> Result<()> {
    let type_check = type_check.into();
    let manifest = read_manifest()?;
    let mut server = start_server()?;
    wait(server_url.clone()).await?;
    apply_from_dev(server_url.clone(), type_check).await;
    let (mut tx, mut rx) = channel(1);
    let mut apply_watcher = RecommendedWatcher::new(move |res: Result<Event, notify::Error>| {
        futures::executor::block_on(async {
            tx.send(res).await.unwrap();
        });
    })?;
    let watcher_config = notify::Config::OngoingEvents(Some(Duration::from_millis(100)));
    apply_watcher.configure(watcher_config.clone())?;

    let mut tracked = HashSet::new();
    let cwd = std::env::current_dir()?;

    for dir in &manifest.models {
        let dir = cwd.join(dir);
        tracked.insert(dir);
    }

    for dir in &manifest.policies {
        let dir = cwd.join(dir);
        tracked.insert(dir);
    }

    for dir in &manifest.endpoints {
        let dir = cwd.join(dir);
        tracked.insert(dir);
    }

    apply_watcher.watch(&cwd, RecursiveMode::Recursive)?;

    while let Some(res) = rx.next().await {
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

                let paths: HashSet<PathBuf> =
                    HashSet::from_iter(paths.into_iter().filter(is_tracked));

                if !paths.is_empty() {
                    apply_from_dev(server_url.clone(), type_check).await;
                }
            }
            Ok(_) => { /* ignore */ }
            Err(e) => eprintln!("watch error: {:?}", e),
        }
    }
    server.wait()?;

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
