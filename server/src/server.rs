// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::datastore::{DbConnection, MetaService, QueryEngine};
use crate::internal::{mark_not_ready, mark_ready};
use crate::kafka;
use crate::opt::Opt;
use crate::policies::PolicySystem;
use crate::trunk::{self, Trunk};
use crate::types::{BuiltinTypes, TypeSystem};
use crate::version::{self, VersionInfo, VersionInit};
use crate::{http, internal, rpc, secrets, worker, JsonObject};
use anyhow::{bail, Context, Result};
use futures::future::{Fuse, FutureExt};
use parking_lot::RwLock;
use regex::Regex;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::panic;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use utils::TaskHandle;

/// Global state of the server.
pub struct Server {
    pub opt: Opt,
    pub db: Arc<DbConnection>,
    pub query_engine: QueryEngine,
    pub meta_service: MetaService,
    /// Global builtin types such as `string` and `AuthUser`, shared for all versions.
    pub builtin_types: Arc<BuiltinTypes>,
    /// Type system for each version (key is version id), should reflect the state of the "meta"
    /// database.
    pub type_systems: tokio::sync::Mutex<HashMap<String, TypeSystem>>,
    /// Current secrets, they are periodically refreshed and rewritten.
    pub secrets: RwLock<JsonObject>,
    /// Handle to an inspector server that allows debugging of JavaScript code from Chrome.
    pub inspector: Option<Arc<deno_runtime::inspector_server::InspectorServer>>,
    /// Trunk with versions ("branches").
    pub trunk: Trunk,
}

pub async fn run(opt: Opt) -> Result<()> {
    // Note that we spawn many tasks, but we .await them all at the end; we never leave a task
    // running in the background. This ensures that we handle all errors and panics and also that
    // we abort the tasks when they are no longer needed (e.g. if other task has failed).
    //
    // This approach is called "structured concurrency", and it seems to be a good way to write
    // concurrent programs and keep your sanity.

    let (server, trunk_task) = make_server(opt).await?;
    start_versions(server.clone()).await?;
    start_builtin_version(server.clone()).await?;

    let (rpc_addr, rpc_task) = rpc::spawn(server.clone(), server.opt.rpc_listen_addr)
        .await
        .context("Could not start gRPC server")?;

    let (http_addrs, http_task) = http::spawn(server.clone(), server.opt.api_listen_addr.clone())
        .await
        .context("Could not start HTTP API server")?;

    let (internal_addr, internal_task) = internal::spawn(server.opt.internal_routes_listen_addr)
        .await
        .context("Could not start an internal HTTP server")?;

    let kafka_task = match server.opt.kafka_connection.clone() {
        Some(connection) => kafka::spawn(server.clone(), connection, &server.opt.kafka_topics)
            .await?
            .fuse(),
        None => Fuse::terminated(),
    };

    let secrets_task = TaskHandle(tokio::task::spawn(refresh_secrets(server.clone())));
    let signal_task = TaskHandle(tokio::task::spawn(wait_for_signals()));

    info!("ChiselStrike server is ready 🚀");
    for http_addr in http_addrs.iter() {
        info!("URL: http://{}", http_addr);
    }
    debug!("gRPC API address: {}", rpc_addr);
    debug!("Internal address: http://{}", internal_addr);
    mark_ready();

    let all_tasks = async move {
        tokio::try_join!(
            trunk_task,
            rpc_task,
            http_task,
            internal_task,
            kafka_task,
            secrets_task
        )
    };
    tokio::select! {
        res = all_tasks => res.map(|_| ()),
        res = signal_task => res,
    }
}

async fn make_server(opt: Opt) -> Result<(Arc<Server>, TaskHandle<Result<()>>)> {
    let db = DbConnection::connect(&opt.db_uri, opt.nr_connections).await?;
    let db = Arc::new(db);
    let query_engine = QueryEngine::new(db.clone());
    let meta_service = MetaService::new(db.clone());

    let legacy_dbs = find_legacy_sqlite_dbs(&opt);
    if extract_sqlite_file(&opt.db_uri).is_some() && legacy_dbs.len() == 2 {
        meta_service
            .maybe_migrate_split_sqlite_database(&legacy_dbs, &opt.db_uri)
            .await
            .context("Could not migrate split sqlite databases into a single database")?;
    }

    meta_service
        .migrate_schema()
        .await
        .context("Could not migrate database schema to the latest version")?;

    let builtin_types = Arc::new(BuiltinTypes::new());
    builtin_types.create_backing_tables(&query_engine).await?;

    let type_systems = meta_service.load_type_systems(&builtin_types).await?;
    let type_systems = tokio::sync::Mutex::new(type_systems);

    let secrets = match secrets::get_secrets(&opt).await {
        Ok(secrets) => secrets,
        Err(err) => {
            log::error!("Could not read secrets: {:?}", err);
            JsonObject::default()
        }
    };
    let secrets = RwLock::new(secrets);

    worker::set_v8_flags(&opt.v8_flags)?;
    let inspector = start_inspector(&opt).await?;

    let (trunk, trunk_task) = trunk::spawn().await?;
    let server = Server {
        opt,
        db,
        query_engine,
        meta_service,
        builtin_types,
        type_systems,
        secrets,
        inspector,
        trunk,
    };
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
        let type_system = type_systems
            .get(&version_id)
            .cloned()
            .unwrap_or_else(|| TypeSystem::new(server.builtin_types.clone(), version_id.clone()));
        let policy_system = server.meta_service.load_policy_system(&version_id).await?;
        let modules = server.meta_service.load_modules(&version_id).await?;

        let root_url = "file:///__root.ts";
        if !modules.contains_key(root_url) {
            warn!(
                "Version {:?} does not contain module {:?}, it was probably created by an old \
                chisel version. This version will be skipped, please rerun `chisel apply` to fix \
                this problem.",
                version_id, root_url,
            );
            continue;
        }

        // ignore the notification that the version is ready
        let (ready_tx, _ready_rx) = oneshot::channel();

        let init = VersionInit {
            version_id,
            info,
            server: server.clone(),
            modules: Arc::new(modules),
            type_system: Arc::new(type_system),
            policy_system: Arc::new(policy_system),
            worker_count: server.opt.worker_threads,
            ready_tx,
        };

        let (version, job_tx, version_task) = version::spawn(init).await?;
        server.trunk.add_version(version, job_tx, version_task);
    }
    Ok(())
}

async fn start_builtin_version(server: Arc<Server>) -> Result<()> {
    let version_id = "__chiselstrike".to_string();
    let info = VersionInfo {
        name: "ChiselStrike Internal API".into(),
        tag: env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT").into(),
    };
    let type_system = TypeSystem::new(server.builtin_types.clone(), version_id.clone());
    let policy_system = PolicySystem::default();

    let mut modules = HashMap::new();
    modules.insert(
        "file:///__root.ts".into(),
        r"
        export * from 'chisel:///builtin_root.ts';
        "
        .into(),
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

    let (version, job_tx, version_task) = version::spawn(init).await?;
    server.trunk.add_version(version, job_tx, version_task);
    Ok(())
}

async fn refresh_secrets(server: Arc<Server>) -> Result<()> {
    let mut last_try_was_failure = false;
    loop {
        if let Err(err) = update_secrets(&server).await {
            if !last_try_was_failure {
                log::warn!("Could not re-read secrets: {:?}", err);
            }
            last_try_was_failure = true;
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }
}

pub async fn update_secrets(server: &Server) -> Result<()> {
    let secrets = secrets::get_secrets(&server.opt).await?;
    *server.secrets.write() = secrets;
    Ok(())
}

async fn wait_for_signals() -> Result<()> {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_hook(info);
        nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
    }));

    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sighup = signal(SignalKind::hangup())?;

    tokio::select! {
        Some(_) = sigterm.recv() => { debug!("Got SIGTERM") },
        Some(_) = sigint.recv() => { debug!("Got SIGINT") },
        Some(_) = sighup.recv() => { debug!("Got SIGHUP") },
    };
    mark_not_ready();
    Ok(())
}

async fn start_inspector(
    opt: &Opt,
) -> Result<Option<Arc<deno_runtime::inspector_server::InspectorServer>>> {
    Ok(if opt.inspect || opt.inspect_brk {
        let addr = alloc_inspector_addr()
            .await
            .context("Could not allocate an address for V8 inspector")?;
        let inspector =
            deno_runtime::inspector_server::InspectorServer::new(addr, "chiseld".into());
        Some(Arc::new(inspector))
    } else {
        None
    })
}

async fn alloc_inspector_addr() -> Result<SocketAddr> {
    use std::io::ErrorKind;
    for port in 9222..9300 {
        match tokio::net::TcpListener::bind(("localhost", port)).await {
            Ok(listener) => return Ok(listener.local_addr()?),
            Err(err) => match err.kind() {
                ErrorKind::AddrInUse | ErrorKind::AddrNotAvailable => {}
                _ => bail!(err),
            },
        }
    }
    bail!("Could not find an available port")
}
