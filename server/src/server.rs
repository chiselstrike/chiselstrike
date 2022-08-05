// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::{api, internal, rpc, secrets, JsonObject};
use crate::datastore::{DbConnection, MetaService, QueryEngine};
use crate::opt::Opt;
use crate::policies::PolicySystem;
use crate::trunk::{self, Trunk};
use crate::types::{BuiltinTypes, TypeSystem};
use crate::version::{self, VersionInfo, VersionInit};
use anyhow::Result;
use parking_lot::RwLock;
use regex::Regex;
use std::panic;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use utils::TaskHandle;

pub struct Server {
    pub db: Arc<DbConnection>,
    pub query_engine: QueryEngine,
    pub meta_service: MetaService,
    pub builtin_types: Arc<BuiltinTypes>,
    pub type_systems: tokio::sync::Mutex<HashMap<String, TypeSystem>>,
    pub secrets: RwLock<JsonObject>,
    pub trunk: Trunk,
}

#[derive(Debug, Copy, Clone)]
pub enum Restart { Yes, No }

pub async fn run(opt: Opt) -> Result<Restart> {
    let (server, trunk_task) = make_server(&opt).await?;
    start_versions(server.clone()).await?;
    start_chiselstrike_version(server.clone()).await?;
    let (rpc_addr, rpc_task) = rpc::spawn(server.clone(), opt.rpc_listen_addr).await?;
    let (api_addrs, api_task) = api::spawn(server.clone(), opt.api_listen_addr).await?;
    let (internal_addr, internal_task) = internal::spawn(
        opt.internal_routes_listen_addr,
        opt.webui,
        rpc_addr,
    ).await?;
    let secrets_task = TaskHandle(tokio::task::spawn(refresh_secrets(server.clone())));
    let signal_task = TaskHandle(tokio::task::spawn(wait_for_signals()));

    info!("ChiselStrike is ready 🚀");
    for api_addr in api_addrs.iter() {
        info!("URL: http://{}", api_addr);
    }
    debug!("gRPC API address: {}", rpc_addr);
    debug!("Internal address: http://{}", internal_addr);

    let all_tasks = async move {
        tokio::try_join!(trunk_task, rpc_task, api_task, internal_task, secrets_task)
    };
    tokio::select! {
        res = all_tasks => res.map(|_| Restart::No),
        res = signal_task => res,
    }
}

async fn make_server(opt: &Opt) -> Result<(Arc<Server>, TaskHandle<Result<()>>)> {
    let db = DbConnection::connect(&opt.db_uri, opt.nr_connections).await?;
    let db = Arc::new(db);
    let query_engine = QueryEngine::new(db.clone());
    let meta_service = MetaService::new(db.clone());

    let legacy_dbs = find_legacy_sqlite_dbs(&opt);
    if extract_sqlite_file(&opt.db_uri).is_some() && legacy_dbs.len() == 2 {
        meta_service.maybe_migrate_sqlite_database(&legacy_dbs, &opt.db_uri).await?;
    }

    meta_service.create_schema().await?;

    let builtin_types = Arc::new(BuiltinTypes::new());
    builtin_types.create_builtin_backing_tables(&query_engine).await?;

    let type_systems = meta_service.load_type_systems(&builtin_types).await?;
    let type_systems = tokio::sync::Mutex::new(type_systems);

    let secrets = match secrets::get_secrets().await {
        Ok(secrets) => secrets,
        Err(err) => {
            log::error!("Could not read secrets: {:?}", err);
            JsonObject::default()
        },
    };
    let secrets = RwLock::new(secrets);

    let (trunk, trunk_task) = trunk::spawn().await?;
    let server = Server { db, query_engine, meta_service, builtin_types, type_systems, secrets, trunk };
    Ok((Arc::new(server), trunk_task))
}

fn find_legacy_sqlite_dbs(opt: &Opt) -> Vec<PathBuf> {
    let mut sources = vec![];
    if let Some(x) = extract_sqlite_file(&opt._metadata_db_uri) {
        sources.push(PathBuf::from(x));
    }
    if let Some(x) = extract_sqlite_file(&opt._data_db_uri) {
        sources.push(PathBuf::from(x));
    }
    sources
}

fn extract_sqlite_file(db_uri: &str) -> Option<String> {
    let regex = Regex::new("^sqlite://(?P<fname>[^?]+)").unwrap();
    regex
        .captures(db_uri)
        .map(|caps| caps.name("fname").unwrap().as_str().to_string())
}

async fn start_versions(server: Arc<Server>) -> Result<()> {
    let version_infos = server.meta_service.load_version_infos().await?;
    let type_systems = server.type_systems.lock().await;
    for (version_id, info) in version_infos.into_iter() {
        let type_system = type_systems.get(&version_id).cloned()
            .unwrap_or_else(|| TypeSystem::new(server.builtin_types.clone(), version_id.clone()));
        let policy_system = server.meta_service.load_policy_system(&version_id).await?;
        let modules = server.meta_service.load_modules(&version_id).await?;

        // ignore the notification that the version is ready
        let (ready_tx, _ready_rx) = oneshot::channel();

        let init = VersionInit {
            version_id,
            info,
            server: server.clone(),
            modules: Arc::new(modules),
            type_system: Arc::new(type_system),
            policy_system: Arc::new(policy_system),
            worker_count: 1,
            ready_tx,
        };

        let (version, version_task) = version::spawn(init).await?;
        server.trunk.add_version(version, version_task);
    }
    Ok(())
}

async fn start_chiselstrike_version(server: Arc<Server>) -> Result<()> {
    let version_id = "__chiselstrike".to_string();
    let info = VersionInfo {
        name: "ChiselStrike Internal API".into(),
        tag: env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT").into(),
    };
    let type_system = TypeSystem::new(server.builtin_types.clone(), version_id.clone());
    let policy_system = PolicySystem::default();

    let mut modules = HashMap::new();
    modules.insert(
        "file:///__route_map.ts".into(),
        "export { default } from 'chisel:///__chiselstrike.ts';".into(),
    );

    let (ready_tx, _ready_rx) = oneshot::channel();

    let init = VersionInit {
        version_id,
        info,
        server: server.clone(),
        modules: Arc::new(modules),
        type_system: Arc::new(type_system),
        policy_system: Arc::new(policy_system),
        worker_count: 1,
        ready_tx,
    };

    let (version, version_task) = version::spawn(init).await?;
    server.trunk.add_version(version, version_task);
    Ok(())
}

async fn refresh_secrets(server: Arc<Server>) -> Result<()> {
    let mut last_try_was_failure = false;
    loop {
        match secrets::get_secrets().await {
            Ok(secrets) => {
                *server.secrets.write() = secrets;
            },
            Err(err) => {
                if !last_try_was_failure {
                    log::warn!("Could not re-read secrets: {:?}", err);
                }
                last_try_was_failure = true;
            },
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }
}

async fn wait_for_signals() -> Result<Restart> {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_hook(info);
        nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
    }));

    use tokio::signal::unix::{SignalKind, signal};
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sighup = signal(SignalKind::hangup())?;
    let mut sigusr1 = signal(SignalKind::user_defined1())?;

    Ok(tokio::select! {
        Some(_) = sigterm.recv() => { debug!("Got SIGTERM"); Restart::No },
        Some(_) = sigint.recv() => { debug!("Got SIGINT"); Restart::No },
        Some(_) = sighup.recv() => { debug!("Got SIGHUP"); Restart::No },
        Some(_) = sigusr1.recv() => { debug!("Got SIGUSR1"); Restart::Yes },
    })
}
