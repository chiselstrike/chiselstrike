// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use deno_broadcast_channel::InMemoryBroadcastChannel;
use deno_core::NoopModuleLoader;
use deno_runtime::permissions::Permissions;
use deno_runtime::worker::{MainWorker, WorkerOptions};
use deno_web::BlobStore;
use hyper::{Body, Response, StatusCode};
use rusty_v8 as v8;
use std::cell::RefCell;
use std::convert::TryInto;
use std::rc::Rc;
use std::sync::Arc;
use url::Url;
use v8::Handle;

/// A v8 isolate doesn't want to be moved between or used from
/// multiple threads. A JsRuntime owns an isolate, so we need to use a
/// thread local storage.
///
/// This has an interesting implication: We cannot easily provide a way to
/// hold transient server state, since each request can hit a different
/// thread. A client that wants that would have to put the information in
/// a database or cookie as appropriate.
///
/// The above is probably fine, since at some point we will be
/// sharding our server, so there is not going to be a single process
/// anyway.
struct DenoService {
    worker: MainWorker,
}

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error["Endpoint didn't produce a response"]]
    NotAResponse,
}

impl DenoService {
    pub fn new() -> Self {
        let create_web_worker_cb = Arc::new(|_| {
            todo!("Web workers are not supported");
        });
        let module_loader = Rc::new(NoopModuleLoader);
        let opts = WorkerOptions {
            apply_source_maps: false,
            args: vec![],
            debug_flag: false,
            unstable: false,
            enable_testing_features: false,
            unsafely_ignore_certificate_errors: None,
            root_cert_store: None,
            user_agent: "hello_runtime".to_string(),
            seed: None,
            js_error_create_fn: None,
            create_web_worker_cb,
            maybe_inspector_server: None,
            should_break_on_first_statement: false,
            module_loader,
            runtime_version: "x".to_string(),
            ts_version: "x".to_string(),
            no_color: true,
            get_error_class_fn: None,
            location: None,
            origin_storage_dir: None,
            blob_store: BlobStore::default(),
            broadcast_channel: InMemoryBroadcastChannel::default(),
            shared_array_buffer_store: None,
            cpu_count: 1,
        };

        let path = "file:///no/such/file";

        let permissions = Permissions {
            read: Permissions::new_read(&Some(vec![path.into()]), false),
            ..Permissions::default()
        };

        let mut worker = MainWorker::from_options(Url::parse(path).unwrap(), permissions, &opts);
        worker.bootstrap(&opts);

        Self { worker }
    }
}

thread_local! {
    static DENO: RefCell<DenoService> = RefCell::new(DenoService::new())
}

pub fn run_js(path: &str, code: &str) -> Result<Response<Body>> {
    DENO.with(|d| -> Result<Response<Body>> {
        let r = &mut d.borrow_mut().worker.js_runtime;
        let res = r.execute_script(path, code)?;
        let scope = &mut r.handle_scope();
        let response = res.get(scope).to_object(scope).ok_or(Error::NotAResponse)?;

        let key = v8::String::new(scope, "text").unwrap();
        let text: v8::Local<v8::Function> = (*response)
            .get(scope, key.into())
            .ok_or(Error::NotAResponse)?
            .try_into()?;
        let text: v8::Local<v8::Promise> = text
            .call(scope, response.into(), &[])
            .ok_or(Error::NotAResponse)?
            .try_into()?;
        let text = text.get(scope).result(scope);

        let key = v8::String::new(scope, "status").unwrap();
        let status: v8::Local<v8::Number> = (*response)
            .get(scope, key.into())
            .ok_or(Error::NotAResponse)?
            .try_into()?;
        let status = status.value() as u16;
        let body = Response::builder()
            .status(StatusCode::from_u16(status)?)
            .body(text.to_rust_string_lossy(scope).into())?;
        Ok(body)
    })
}
