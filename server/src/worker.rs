// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::datastore::engine::TransactionStatic;
use crate::module_loader::ModuleLoader;
use crate::ops;
use crate::server::Server;
use crate::version::{Version, VersionJob};
use anyhow::{bail, Context as _, Result};
use deno_core::url::Url;
use futures::ready;
use std::collections::HashMap;
use std::future::Future;
use std::iter::once;
use std::panic;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::{mpsc, oneshot};
use utils::TaskHandle;

pub struct WorkerInit {
    pub worker_idx: usize,
    pub server: Arc<Server>,
    pub version: Arc<Version>,
    /// Module map (see `ModuleLoader`).
    pub modules: Arc<HashMap<String, String>>,
    /// The worker will signal on this channel when it is ready to accept jobs.
    pub ready_tx: oneshot::Sender<()>,
    /// The worker will receive jobs from this channel.
    pub job_rx: mpsc::Receiver<VersionJob>,
}

/// Handle to a worker task and thread.
///
/// The worker runs as a task in a tokio local set on a dedicated thread. This struct conveniently
/// bundles the handles to the task and the thread, so that we can join the thread when the task
/// finishes.
#[derive(Debug)]
pub struct WorkerJoinHandle {
    task: TaskHandle<Result<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// State of one worker (JavaScript runtime).
///
/// This struct is stored in the `op_state` in the Deno runtime, from where we can obtain it in
/// Deno ops. Every worker runs on its own thread and runs code for a single version.
pub struct WorkerState {
    pub server: Arc<Server>,
    pub version: Arc<Version>,

    /// The implicit global transaction for all data operations.
    ///
    /// TODO: the existence of this transaction means that the worker can only handle a single
    /// job at a time. Unfortunately, to get rid of this, we have to significantly rework the
    /// TypeScript API.
    pub transaction: Option<TransactionStatic>,

    /// Channel for signaling that the worker is ready to handle jobs.
    ///
    /// Once the worker sends the signal, this is reset to `None`.
    pub ready_tx: Option<oneshot::Sender<()>>,

    /// Channel for receiving jobs.
    ///
    /// To wait on this channel, we temporarily move out of the `Option`.
    pub job_rx: Option<mpsc::Receiver<VersionJob>>,
}

pub async fn spawn(init: WorkerInit) -> Result<WorkerJoinHandle> {
    let runtime_handle = tokio::runtime::Handle::try_current().unwrap();
    let (task_tx, task_rx) = oneshot::channel();

    // spawn the worker task in a localset on a new dedicated thread
    let thread = std::thread::spawn(move || {
        let local_set = tokio::task::LocalSet::new();
        let task = local_set.spawn_local(run(init));
        let _ = task_tx.send(task);
        runtime_handle.block_on(local_set)
    });

    let task = TaskHandle(task_rx.await.unwrap());
    Ok(WorkerJoinHandle {
        task,
        thread: Some(thread),
    })
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
        location: Some(Url::parse("http://chiselstrike.com").unwrap()),
        no_color: true,
        runtime_version: "x".to_string(),
        ts_version: "x".to_string(),
        unstable: true,
    };

    let extensions = vec![ops::extension()];
    let module_loader = Rc::new(ModuleLoader::new(init.modules));
    let create_web_worker_cb = Arc::new(|_| panic!("Web workers are not supported"));
    let web_worker_preload_module_cb = Arc::new(|_| panic!("Web workers are not supported"));
    let web_worker_pre_execute_module_cb = Arc::new(|_| panic!("Web workers are not supported"));

    let options = deno_runtime::worker::WorkerOptions {
        bootstrap,
        extensions,
        unsafely_ignore_certificate_errors: None,
        root_cert_store: None,
        seed: None,
        module_loader,
        npm_resolver: None,
        create_web_worker_cb,
        web_worker_preload_module_cb,
        web_worker_pre_execute_module_cb,
        format_js_error_fn: None,
        source_map_getter: None,
        maybe_inspector_server: init.server.inspector.clone(),
        should_break_on_first_statement: init.server.opt.inspect_brk,
        get_error_class_fn: Some(&get_error_class_name),
        origin_storage_dir: None,
        blob_store: Default::default(),
        broadcast_channel: Default::default(),
        shared_array_buffer_store: None,
        compiled_wasm_module_store: None,
        stdio: Default::default(),
    };

    use deno_runtime::permissions::Permissions;
    let permissions = Permissions {
        // FIXME: Temporary hack to allow easier testing for now
        net: Permissions::new_net(&Some(vec![]), false).unwrap(),
        ..Permissions::default()
    };

    let mut main_url = Url::parse("chisel://api/main.js").unwrap();
    // the `main_url` is given to the Deno `InspectorServer` when registering and is visible in
    // `chrome://inspect`, so it is useful to add version and worker index to the URL in order to
    // distinguish between different targets on the same inspector server
    main_url
        .query_pairs_mut()
        .append_pair("version", &init.version.version_id)
        .append_pair("worker", &init.worker_idx.to_string());

    let mut worker = deno_runtime::worker::MainWorker::bootstrap_from_options(
        main_url.clone(),
        permissions,
        options,
    );

    let worker_state = WorkerState {
        server: init.server,
        version: init.version.clone(),
        transaction: None,
        ready_tx: Some(init.ready_tx),
        job_rx: Some(init.job_rx),
    };
    worker.js_runtime.op_state().borrow_mut().put(worker_state);

    // start executing the JavaScript code in main.js; this will return when the worker is
    // terminated, any futher interaction with JavaScript is done exclusively using Deno ops
    worker.execute_main_module(&main_url).await.context(format!(
        "Error when executing JavaScript for version {:?} in worker {}",
        init.version.version_id, init.worker_idx
    ))
}

impl Future for WorkerJoinHandle {
    type Output = Result<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = Pin::into_inner(self);
        let task_res = ready!(Pin::new(&mut this.task).poll(cx));
        // when the task has finished, the thread should also finish, so it is safe to do the
        // blocking join() here
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
        .or_else(|| e.downcast_ref::<&'static str>().map(|_| "Error"))
        .unwrap_or_else(|| {
            // when this is printed, please handle the unknown type by adding another
            // `downcast_ref()` check above
            warn!("Unknown error type: {:#?}", e);
            "Error"
        })
}

pub fn set_v8_flags(flags: &[String]) -> Result<()> {
    let v8_flags = once("unused_arg0".to_owned())
        .chain(flags.iter().cloned())
        .collect();
    let unrecognized_v8_flags = deno_core::v8_set_flags(v8_flags);
    if unrecognized_v8_flags.len() > 1 {
        bail!(
            "V8 did not recognize flags: {:?}",
            &unrecognized_v8_flags[1..]
        )
    }
    Ok(())
}
