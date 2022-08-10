// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::ops;
use crate::api::ApiRequestResponse;
use crate::datastore::engine::TransactionStatic;
use crate::server::Server;
use crate::version::Version;
use anyhow::{Context as _, Result};
use deno_core::url::Url;
use futures::ready;
use std::collections::HashMap;
use std::future::Future;
use std::panic;
use std::marker::Unpin;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::oneshot;
use utils::TaskHandle;

mod loader;

pub struct WorkerInit {
    pub server: Arc<Server>,
    pub version: Arc<Version>,
    pub modules: Arc<HashMap<String, String>>,
    pub ready_tx: oneshot::Sender<()>,
    pub request_rx: async_channel::Receiver<ApiRequestResponse>,
}

#[derive(Debug)]
pub struct WorkerJoinHandle {
    task: TaskHandle<Result<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

pub struct WorkerState {
    pub server: Arc<Server>,
    pub version: Arc<Version>,
    pub transaction: Option<TransactionStatic>,
    pub ready_tx: Option<oneshot::Sender<()>>,
    pub request_rx: async_channel::Receiver<ApiRequestResponse>,
}

pub async fn spawn(init: WorkerInit) -> Result<WorkerJoinHandle> {
    let runtime_handle = tokio::runtime::Handle::try_current().unwrap();
    let (task_tx, task_rx) = oneshot::channel();

    let thread = std::thread::spawn(move || {
        let local_set = tokio::task::LocalSet::new();
        let task = local_set.spawn_local(run(init));
        let _ = task_tx.send(task);
        runtime_handle.block_on(local_set)
    });
    
    let task = TaskHandle(task_rx.await.unwrap());
    Ok(WorkerJoinHandle { task, thread: Some(thread) })
}

async fn run(init: WorkerInit) -> Result<()> {
    let bootstrap = deno_runtime::BootstrapOptions {
        user_agent: "chiseld".to_string(),
        args: vec![],
        cpu_count: 1,
        debug_flag: false,
        enable_testing_features: false,
        is_tty: false,
        // FIXME: make location a configuration parameter
        location: Some(Url::parse("https://chiselstrike.com").unwrap()),
        no_color: true,
        runtime_version: "x".to_string(),
        ts_version: "x".to_string(),
        unstable: true,
    };

    let extensions = vec![ops::extension()];
    let module_loader = Rc::new(loader::ModuleLoader::new(init.modules));
    let create_web_worker_cb = Arc::new(|_| {
        panic!("Web workers are not supported")
    });
    let web_worker_preload_module_cb = Arc::new(|_| {
        panic!("Web workers are not supported")
    });

    let options = deno_runtime::worker::WorkerOptions {
        format_js_error_fn: None,
        source_map_getter: None,
        stdio: Default::default(),
        bootstrap,
        extensions,
        unsafely_ignore_certificate_errors: None,
        root_cert_store: None,
        seed: None,
        create_web_worker_cb,
        maybe_inspector_server: None,
        should_break_on_first_statement: false,
        module_loader,
        get_error_class_fn: Some(&get_error_class_name),
        origin_storage_dir: None,
        blob_store: Default::default(),
        broadcast_channel: Default::default(),
        shared_array_buffer_store: None,
        compiled_wasm_module_store: None,
        web_worker_preload_module_cb,
    };

    use deno_runtime::permissions::Permissions;
    let permissions = Permissions {
        // FIXME: Temporary hack to allow easier testing for now
        net: Permissions::new_net(&Some(vec![]), false),
        ..Permissions::default()
    };

    let main_url = Url::parse("chisel:///main.js").unwrap();
    let mut worker = deno_runtime::worker::MainWorker::bootstrap_from_options(
        main_url.clone(), permissions, options,
    );

    let worker_state = WorkerState {
        server: init.server,
        version: init.version.clone(),
        transaction: None,
        ready_tx: Some(init.ready_tx),
        request_rx: init.request_rx,
    };
    worker.js_runtime.op_state().borrow_mut().put(worker_state);

    worker.execute_main_module(&main_url).await
        .context(format!("Error when executing JavaScript for version {:?}", init.version.version_id))
}

impl Unpin for WorkerJoinHandle {}

impl Future for WorkerJoinHandle {
    type Output = Result<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = Pin::into_inner(self);
        let task_res = ready!(Pin::new(&mut this.task).poll(cx));
        let join_res = this.thread.take().unwrap().join();
        Poll::Ready(match (task_res, join_res) {
            (_, Err(join_err)) => panic::resume_unwind(join_err),
            (Err(task_err), _) => Err(task_err),
            (Ok(_), Ok(_)) => Ok(()),
        })
    }
}

fn get_error_class_name(e: &anyhow::Error) -> &'static str {
    // this function is based on `get_error_class_name()` from deno/cli/error.rs
    deno_runtime::errors::get_error_class_name(e)
        .or_else(|| {
            // plain string errors produced by anyhow!("something"), .context("something") and
            // friends
            e.downcast_ref::<String>().map(|_| "Error")
        })
        .or_else(|| {
            e.downcast_ref::<&'static str>().map(|_| "Error")
        })
        .unwrap_or_else(|| {
            // when this is printed, please handle the unknown type by adding another
            // `downcast_ref()` check above
            warn!("Unknown error type: {:#?}", e);
            "Error"
        })
}
