// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::cmd::apply::{apply, AllowTypeDeletion, TypeChecking};
use crate::project::read_manifest;
use crate::server::{start_server, wait};
use crate::DEFAULT_API_VERSION;
use anyhow::Result;
use futures::channel::mpsc::channel;
use futures::{SinkExt, StreamExt};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::time::Duration;

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
    while let Some(res) = rx.next().await {
        match res {
            Ok(notify::event::Event {
                kind: notify::event::EventKind::Modify(notify::event::ModifyKind::Data(_)),
                ..
            }) => {
                apply_from_dev(server_url.clone(), type_check).await;
            }
            Ok(_) => { /* ignore */ }
            Err(e) => println!("watch error: {:?}", e),
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
