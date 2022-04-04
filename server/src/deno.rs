// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::{response_template, Body, RequestPath};
use crate::datastore::engine::IdTree;
use crate::datastore::engine::TransactionStatic;
use crate::datastore::engine::{QueryResults, ResultRow};
use crate::datastore::query::QueryOpChain;
use crate::datastore::query::{Mutation, QueryPlan};
use crate::datastore::QueryEngine;
use crate::policies::FieldPolicies;
use crate::rcmut::RcMut;
use crate::runtime::Runtime;
use crate::types::ObjectType;
use crate::types::Type;
use crate::types::TypeSystem;
use crate::types::TypeSystemError;
use crate::JsonObject;
use anyhow::{anyhow, Context as AnyhowContext, Result};
use api::chisel_js;
use api::endpoint_js;
use deno_core::error::AnyError;
use deno_core::op;
use deno_core::v8;
use deno_core::CancelFuture;
use deno_core::CancelHandle;
use deno_core::Extension;
use deno_core::JsRuntime;
use deno_core::ModuleSource;
use deno_core::ModuleSourceFuture;
use deno_core::ModuleSpecifier;
use deno_core::ModuleType;
use deno_core::OpState;
use deno_core::RcRef;
use deno_core::Resource;
use deno_core::ResourceId;
use deno_core::ZeroCopyBuf;
use deno_runtime::inspector_server::InspectorServer;
use deno_runtime::ops::worker_host::CreateWebWorkerCb;
use deno_runtime::ops::worker_host::PreloadModuleCb;
use deno_runtime::permissions::Permissions;
use deno_runtime::web_worker::WebWorker;
use deno_runtime::web_worker::WebWorkerOptions;
use deno_runtime::worker::{MainWorker, WorkerOptions};
use deno_runtime::BootstrapOptions;
use futures::future;
use futures::stream::{try_unfold, Stream};
use futures::task::LocalFutureObj;
use futures::FutureExt;
use futures::StreamExt;
use hyper::body::HttpBody;
use hyper::Method;
use hyper::Uri;
use hyper::{Request, Response, StatusCode};
use log::debug;
use once_cell::unsync::OnceCell;
use serde_derive::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt::Debug;
use std::future::Future;
use std::io::Write;
use std::net::SocketAddr;
use std::ops::DerefMut;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll};
use tempfile::Builder;

// FIXME: This should not be here. The client should download and
// compile modules, the server should not get code out of the
// internet.
use tsc_compile::compile_ts_code;
use tsc_compile::CompileOptions;

use url::Url;

struct VersionedCode {
    code: String,
    version: u64,
}

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

    // We need a copy to keep it alive
    inspector: Option<Arc<InspectorServer>>,

    module_loader: Arc<std::sync::Mutex<ModuleLoaderInner>>,

    import_endpoint: v8::Global<v8::Function>,
    activate_endpoint: v8::Global<v8::Function>,
    call_handler: v8::Global<v8::Function>,
}

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error["Endpoint didn't produce a response"]]
    NotAResponse,
}

struct ModuleLoaderInner {
    code_map: HashMap<String, VersionedCode>,
}

struct ModuleLoader {
    inner: Arc<std::sync::Mutex<ModuleLoaderInner>>,
}

fn wrap(specifier: &ModuleSpecifier, code: String) -> Result<ModuleSource> {
    Ok(ModuleSource {
        code,
        module_type: ModuleType::JavaScript,
        module_url_specified: specifier.to_string(),
        module_url_found: specifier.to_string(),
    })
}

async fn compile(code: &str, lib: Option<&str>) -> Result<String> {
    let mut f = Builder::new().suffix(".ts").tempfile()?;
    let inner = f.as_file_mut();
    inner.write_all(code.as_bytes())?;
    inner.flush()?;
    let path = f.path().to_str().unwrap();
    let opts = CompileOptions {
        extra_default_lib: lib,
        ..Default::default()
    };
    Ok(compile_ts_code(path, opts).await?.remove(path).unwrap())
}

async fn load_code(code_opt: Option<String>, specifier: ModuleSpecifier) -> Result<ModuleSource> {
    let code = if let Some(code) = code_opt {
        code
    } else {
        let mut code = utils::get_ok(specifier.clone()).await?.text().await?;
        let last = specifier.path_segments().unwrap().rev().next().unwrap();
        if last.ends_with(".ts") {
            code = compile(&code, None).await?;
        }
        code
    };
    wrap(&specifier, code)
}

impl deno_core::ModuleLoader for ModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _is_main: bool,
    ) -> Result<ModuleSpecifier, AnyError> {
        debug!("Deno resolving {:?}", specifier);
        if specifier == "@chiselstrike/api" {
            let api_path = "/chisel.ts".to_string();
            let spec = ModuleSpecifier::from_file_path(&api_path)
                .map_err(|_| anyhow!("Can't convert {} to file-based URL", api_path))?;
            Ok(spec)
        } else {
            Ok(deno_core::resolve_import(specifier, referrer)?)
        }
    }

    fn load(
        &self,
        specifier: &ModuleSpecifier,
        _maybe_referrer: Option<ModuleSpecifier>,
        _is_dyn_import: bool,
    ) -> Pin<Box<ModuleSourceFuture>> {
        let handle = self.inner.lock().unwrap();
        let code = if specifier.scheme() == "file" {
            let path = specifier.to_file_path().unwrap();
            let path = path.to_str().unwrap();
            Some(handle.code_map.get(path).unwrap().code.clone())
        } else {
            None
        };
        load_code(code, specifier.clone()).boxed_local()
    }
}

fn build_extensions() -> Vec<Extension> {
    vec![Extension::builder()
        .ops(vec![
            op_format_file_name::decl(),
            op_chisel_read_body::decl(),
            op_chisel_store::decl(),
            op_chisel_entity_delete::decl(),
            op_chisel_get_secret::decl(),
            op_chisel_relational_query_create::decl(),
            op_chisel_relational_query_next::decl(),
        ])
        .build()]
}

fn create_web_worker(
    bootstrap: BootstrapOptions,
    preload_module_cb: Arc<PreloadModuleCb>,
    maybe_inspector_server: Option<Arc<InspectorServer>>,
    module_loader_inner: Arc<std::sync::Mutex<ModuleLoaderInner>>,
) -> Arc<CreateWebWorkerCb> {
    Arc::new(move |args| {
        let create_web_worker_cb = create_web_worker(
            bootstrap.clone(),
            preload_module_cb.clone(),
            maybe_inspector_server.clone(),
            module_loader_inner.clone(),
        );

        let module_loader = Rc::new(ModuleLoader {
            inner: module_loader_inner.clone(),
        });

        let extensions = build_extensions();

        // FIXME: Send a patch refactoring WebWorkerOptions and WorkerOptions
        let options = WebWorkerOptions {
            bootstrap: bootstrap.clone(),
            extensions,
            unsafely_ignore_certificate_errors: None,
            root_cert_store: None,
            user_agent: "hello_runtime".to_string(),
            seed: None,
            module_loader,
            create_web_worker_cb,
            preload_module_cb: preload_module_cb.clone(),
            js_error_create_fn: None,
            use_deno_namespace: args.use_deno_namespace,
            worker_type: args.worker_type,
            maybe_inspector_server: maybe_inspector_server.clone(),
            get_error_class_fn: None,
            blob_store: Default::default(),
            broadcast_channel: Default::default(),
            shared_array_buffer_store: None,
            compiled_wasm_module_store: None,
            maybe_exit_code: args.maybe_exit_code,
        };
        WebWorker::bootstrap_from_options(
            args.name,
            args.permissions,
            args.main_module,
            args.worker_id,
            options,
        )
    })
}

impl DenoService {
    pub(crate) async fn new(inspect_brk: bool) -> Self {
        let web_worker_preload_module_cb =
            Arc::new(|worker| LocalFutureObj::new(Box::new(future::ready(Ok(worker)))));
        let inner = Arc::new(std::sync::Mutex::new(ModuleLoaderInner {
            code_map: HashMap::new(),
        }));
        let module_loader = Rc::new(ModuleLoader {
            inner: inner.clone(),
        });

        let mut inspector = None;
        if inspect_brk {
            let addr: SocketAddr = "127.0.0.1:9229".parse().unwrap();
            inspector = Some(Arc::new(InspectorServer::new(addr, "chisel".to_string())));
        }

        let bootstrap = BootstrapOptions {
            apply_source_maps: false,
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
        let extensions = build_extensions();
        let create_web_worker_cb = create_web_worker(
            bootstrap.clone(),
            web_worker_preload_module_cb.clone(),
            inspector.clone(),
            inner.clone(),
        );
        let opts = WorkerOptions {
            bootstrap,
            extensions,
            unsafely_ignore_certificate_errors: None,
            root_cert_store: None,
            user_agent: "hello_runtime".to_string(),
            seed: None,
            js_error_create_fn: None,
            create_web_worker_cb,
            maybe_inspector_server: inspector.clone(),
            should_break_on_first_statement: false,
            module_loader,
            get_error_class_fn: None,
            origin_storage_dir: None,
            blob_store: Default::default(),
            broadcast_channel: Default::default(),
            shared_array_buffer_store: None,
            compiled_wasm_module_store: None,
            web_worker_preload_module_cb,
        };

        let path = "file:///no/such/file";

        let permissions = Permissions {
            read: Permissions::new_read(&Some(vec![path.into()]), false),
            // FIXME: Temporary hack to allow easier testing for
            // now. Which network access is allowed should be a
            // configured with the endpoint.
            net: Permissions::new_net(&Some(vec![]), false),
            ..Permissions::default()
        };

        let mut worker =
            MainWorker::bootstrap_from_options(Url::parse(path).unwrap(), permissions, opts);

        let main_path = "/main.js";
        let endpoint_path = "/endpoint.ts";
        {
            let mut handle = inner.lock().unwrap();
            let code_map = &mut handle.code_map;
            code_map.insert(
                main_path.to_string(),
                VersionedCode {
                    code: include_str!("./main.js").to_string(),
                    version: 0,
                },
            );

            code_map.insert(
                "/chisel.ts".to_string(),
                VersionedCode {
                    code: chisel_js().to_string(),
                    version: 0,
                },
            );
            code_map.insert(
                endpoint_path.to_string(),
                VersionedCode {
                    code: endpoint_js().to_string(),
                    version: 0,
                },
            );
        }

        worker
            .execute_main_module(&ModuleSpecifier::parse(&format!("file://{}", main_path)).unwrap())
            .await
            .unwrap();

        let (import_endpoint, activate_endpoint, call_handler) = {
            let runtime = &mut worker.js_runtime;
            let promise = runtime
                .execute_script(main_path, &format!("import(\"file://{}\")", endpoint_path))
                .unwrap();
            let module = runtime.resolve_value(promise).await.unwrap();
            let scope = &mut runtime.handle_scope();
            let module = v8::Local::new(scope, module).try_into().unwrap();
            let import_endpoint: v8::Local<v8::Function> =
                get_member(module, scope, "importEndpoint").unwrap();
            let import_endpoint = v8::Global::new(scope, import_endpoint);
            let activate_endpoint: v8::Local<v8::Function> =
                get_member(module, scope, "activateEndpoint").unwrap();
            let activate_endpoint = v8::Global::new(scope, activate_endpoint);
            let call_handler: v8::Local<v8::Function> =
                get_member(module, scope, "callHandler").unwrap();
            let call_handler = v8::Global::new(scope, call_handler);
            (import_endpoint, activate_endpoint, call_handler)
        };

        Self {
            worker,
            inspector,
            module_loader: inner,
            import_endpoint,
            activate_endpoint,
            call_handler,
        }
    }
}

// A future that resolves the hyper::Body has data.
struct ReadFuture {
    resource: Rc<BodyResource>,
}

impl Future for ReadFuture {
    type Output = Option<Result<hyper::body::Bytes, hyper::Error>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut borrow = self.resource.body.borrow_mut();
        let body: &mut hyper::Body = &mut borrow;
        Pin::new(body).poll_data(cx)
    }
}

#[op]
async fn op_chisel_read_body(
    state: Rc<RefCell<OpState>>,
    body_rid: ResourceId,
) -> Result<Option<ZeroCopyBuf>> {
    let resource: Rc<BodyResource> = state.borrow().resource_table.get(body_rid)?;
    let cancel = RcRef::map(&resource, |r| &r.cancel);
    let fut = ReadFuture {
        resource: resource.clone(),
    };
    let fut = fut.or_cancel(cancel);
    Ok(fut.await?.transpose()?.map(|x| x.to_vec().into()))
}

#[derive(Deserialize)]
struct StoreContent {
    name: String,
    value: JsonObject,
}

#[op]
async fn op_chisel_store(
    state: Rc<RefCell<OpState>>,
    content: StoreContent,
    api_version: String,
) -> Result<IdTree> {
    let type_name = &content.name;
    let value = &content.value;

    let (query_engine, ty) = {
        // Users can only store custom types.  Builtin types are managed by us.
        let mut state = state.borrow_mut();
        let ty = match current_type_system(&mut state).lookup_custom_type(type_name, &api_version) {
            Err(_) => anyhow::bail!("Cannot save into type {}.", type_name),
            Ok(ty) => ty,
        };

        let query_engine = query_engine(&mut state).clone();
        (query_engine, ty)
    };
    let transaction = {
        let state = state.borrow();
        current_transaction(&state)?
    };
    let mut transaction = transaction.lock().await;
    Ok(query_engine
        .add_row(&ty, value, Some(transaction.deref_mut()))
        .await?)
}

#[derive(Deserialize)]
struct DeleteContent {
    type_name: String,
    restrictions: JsonObject,
}

#[op]
async fn op_chisel_entity_delete(
    state: Rc<RefCell<OpState>>,
    content: DeleteContent,
    api_version: String,
) -> Result<()> {
    let mutation = {
        let mut state = state.borrow_mut();
        Mutation::parse_delete(
            current_type_system(&mut state),
            &api_version,
            &content.type_name,
            &content.restrictions,
        )
        .context(
            "failed to construct delete expression from JSON passed to `op_chisel_entity_delete`",
        )?
    };
    let query_engine = {
        let mut state = state.borrow_mut();
        query_engine(&mut state).clone()
    };
    query_engine.mutate(mutation).await
}

type DbStream = RefCell<QueryResults>;

/// Calculates field policies for the request being processed.
pub(crate) fn make_field_policies(
    runtime: &Runtime,
    userid: &Option<String>,
    path: &str,
    ty: &ObjectType,
) -> FieldPolicies {
    let mut policies = FieldPolicies {
        current_userid: userid.clone(),
        ..Default::default()
    };
    runtime.add_field_policies(ty, &mut policies, path);
    policies
}

struct QueryStreamResource {
    stream: DbStream,
}

impl Resource for QueryStreamResource {}

#[op]
fn op_chisel_get_secret(op_state: &mut OpState, key: String) -> Result<Option<serde_json::Value>> {
    let ret = if let Some(secrets) = current_secrets(op_state) {
        secrets.get(&key).cloned()
    } else {
        None
    };
    Ok(ret)
}

#[op]
fn op_chisel_relational_query_create(
    op_state: &mut OpState,
    op_chain: QueryOpChain,
    info: (String, String, Option<String>),
) -> Result<ResourceId> {
    let (api_version, path, userid) = info;
    let query_plan = QueryPlan::from_op_chain(
        current_type_system(op_state),
        &api_version,
        &userid,
        &path,
        op_chain,
    )?;
    let transaction = current_transaction(op_state)?;
    let query_engine = query_engine(op_state);
    let stream = query_engine.query(transaction, query_plan)?;
    let resource = QueryStreamResource {
        stream: RefCell::new(stream),
    };
    let rid = op_state.resource_table.add(resource);
    Ok(rid)
}

// A future that resolves when this stream next element is available.
struct QueryNextFuture {
    resource: Rc<QueryStreamResource>,
}

impl Future for QueryNextFuture {
    type Output = Option<Result<ResultRow>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut stream = self.resource.stream.borrow_mut();
        let stream: &mut QueryResults = &mut stream;
        Pin::new(stream).poll_next(cx)
    }
}

#[op]
async fn op_chisel_relational_query_next(
    state: Rc<RefCell<OpState>>,
    query_stream_rid: ResourceId,
) -> Result<Option<ResultRow>> {
    let resource: Rc<QueryStreamResource> = state.borrow().resource_table.get(query_stream_rid)?;
    let fut = QueryNextFuture { resource };
    if let Some(row) = fut.await {
        Ok(Some(row?))
    } else {
        Ok(None)
    }
}

// Used by deno to format names in errors
#[op]
fn op_format_file_name(file_name: String) -> Result<String> {
    Ok(file_name)
}

pub(crate) async fn init_deno(inspect_brk: bool) -> Result<()> {
    let service = Rc::new(RefCell::new(DenoService::new(inspect_brk).await));
    DENO.with(|d| {
        d.set(service)
            .map_err(|_| ())
            .expect("Deno is already initialized.");
    });
    Ok(())
}

thread_local! {
    // There is no 'thread lifetime in rust. So without Rc we can't
    // convince rust that a future produced with DENO.with doesn't
    // outlive the DenoService.
    static DENO: OnceCell<Rc<RefCell<DenoService>>> = OnceCell::new();
}

fn try_into_or<'s, T: std::convert::TryFrom<v8::Local<'s, v8::Value>>>(
    val: Option<v8::Local<'s, v8::Value>>,
) -> Result<T>
where
    T::Error: std::error::Error + Send + Sync + 'static,
{
    Ok(val.ok_or(Error::NotAResponse)?.try_into()?)
}

fn get_member<'a, T: std::convert::TryFrom<v8::Local<'a, v8::Value>>>(
    obj: v8::Local<v8::Object>,
    scope: &mut v8::HandleScope<'a>,
    key: &str,
) -> Result<T>
where
    T::Error: std::error::Error + Send + Sync + 'static,
{
    let key = v8::String::new(scope, key).unwrap();
    let res: T = try_into_or(obj.get(scope, key.into()))?;
    Ok(res)
}

// A future that resolves when the js promise is fulfilled.
struct ResolveFuture {
    js_promise: v8::Global<v8::Value>,
}

impl Future for ResolveFuture {
    type Output = Result<v8::Global<v8::Value>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut service = get();
        let runtime = &mut service.worker.js_runtime;
        let ret = runtime.poll_value(&self.js_promise, cx);
        if ret.is_pending() {
            // FIXME: This a hack around
            // https://github.com/denoland/deno/issues/13458 We call
            // wake more often than needed, but at least this prevents
            // us from stalling.
            cx.waker().clone().wake();
        }
        ret
    }
}

fn resolve_promise(
    js_promise: v8::Global<v8::Value>,
) -> impl Future<Output = Result<v8::Global<v8::Value>>> {
    ResolveFuture { js_promise }
}

async fn get_read_future(
    read_tpl: Option<v8::Global<v8::Function>>,
) -> Result<Option<(Box<[u8]>, ())>> {
    let read = match read_tpl {
        Some(x) => x,
        None => {
            return Ok(None);
        }
    };

    let js_promise = {
        let mut service = get();
        let runtime = &mut service.worker.js_runtime;
        let scope = &mut runtime.handle_scope();
        let und = v8::undefined(scope).into();
        let res = read
            .open(scope)
            .call(scope, und, &[])
            .ok_or(Error::NotAResponse)?;
        v8::Global::new(scope, res)
    };
    let read_result = resolve_promise(js_promise).await?;
    let mut service = get();
    let runtime = &mut service.worker.js_runtime;
    let scope = &mut runtime.handle_scope();
    let read_result = v8::Local::new(scope, read_result);
    let value: v8::Local<v8::ArrayBufferView> = match read_result.try_into() {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let size = value.byte_length();
    // FIXME: We might want to use an uninitialized buffer.
    let mut buffer = vec![0; size];
    let copied = value.copy_contents(&mut buffer);
    // FIXME: Check in V8 to see when this might fail
    assert!(copied == size);
    Ok(Some((buffer.into_boxed_slice(), ())))
}

fn get_read_stream(
    runtime: &mut JsRuntime,
    global_response: v8::Global<v8::Value>,
) -> Result<impl Stream<Item = Result<Box<[u8]>>>> {
    let scope = &mut runtime.handle_scope();
    let response = global_response
        .open(scope)
        .to_object(scope)
        .ok_or(Error::NotAResponse)?;

    let read = match get_member::<v8::Local<v8::Function>>(response, scope, "read") {
        Ok(read) => {
            let read = v8::Global::new(scope, read);
            Some(read)
        }
        Err(_) => None,
    };

    Ok(try_unfold((), move |_| get_read_future(read.clone())))
}

struct BodyResource {
    body: RefCell<hyper::Body>,
    cancel: CancelHandle,
}

impl Resource for BodyResource {
    fn close(self: Rc<Self>) {
        self.cancel.cancel();
    }
}

fn with_op_state<T, F>(func: F) -> T
where
    F: FnOnce(&mut OpState) -> T,
{
    let mut service = get();
    let runtime = &mut service.worker.js_runtime;
    let op_state = runtime.op_state();
    let mut borrow = op_state.borrow_mut();
    func(&mut borrow)
}

fn current_transaction(st: &OpState) -> Result<TransactionStatic> {
    st.try_borrow()
        .cloned()
        .ok_or_else(|| anyhow!("no active transaction"))
}

fn take_current_transaction() -> Option<TransactionStatic> {
    with_op_state(|state| {
        // FIXME: Return a Result once all concurrency issues are fixed.
        state.try_take()
    })
}

fn set_current_transaction(st: &mut OpState, transaction: TransactionStatic) {
    st.put(transaction);
}

fn current_secrets(st: &OpState) -> Option<&JsonObject> {
    st.try_borrow()
}

fn set_current_secrets(st: &mut OpState, secrets: JsonObject) {
    st.put(secrets);
}

fn current_type_system(st: &mut OpState) -> &mut TypeSystem {
    st.borrow_mut()
}

fn query_engine(st: &mut OpState) -> &mut Arc<QueryEngine> {
    st.borrow_mut()
}

pub(crate) fn set_query_engine(query_engine: Arc<QueryEngine>) {
    with_op_state(move |state| {
        state.put(query_engine);
    });
}

pub(crate) fn query_engine_arc() -> Arc<QueryEngine> {
    with_op_state(|state| query_engine(state).clone())
}

pub(crate) fn lookup_builtin_type(type_name: &str) -> Result<Type, TypeSystemError> {
    with_op_state(|state| {
        let type_system = current_type_system(state);
        type_system.lookup_builtin_type(type_name)
    })
}

pub(crate) fn remove_type_version(version: &str) {
    with_op_state(|state| {
        let type_system = current_type_system(state);
        type_system.versions.remove(version);
    });
}

pub(crate) fn set_type_system(type_system: TypeSystem) {
    with_op_state(move |state| {
        state.put(type_system);
    });
}

pub(crate) fn update_secrets(secrets: JsonObject) {
    with_op_state(|state| {
        set_current_secrets(state, secrets);
    })
}

fn get_result_aux(
    path: RequestPath,
    userid: &Option<String>,
    req: Request<hyper::Body>,
) -> Result<v8::Global<v8::Value>> {
    let mut service = get();
    let service: &mut DenoService = &mut service;
    let runtime = &mut service.worker.js_runtime;

    let op_state = runtime.op_state();
    let scope = &mut runtime.handle_scope();

    // Hyper gives us a URL with just the path, make it a full URL
    // before passing it to deno.
    // FIXME: Use the real values for this server.
    let url = Uri::builder()
        .scheme("http")
        .authority("chiselstrike.com")
        .path_and_query(req.uri().path_and_query().unwrap().clone())
        .build()
        .unwrap();
    // FIXME: this request conversion is probably simplistic. Check deno/ext/http/lib.rs
    let url = v8::String::new(scope, &url.to_string()).unwrap();
    let method = req.method();
    let method_value = v8::String::new(scope, method.as_str()).unwrap().into();

    let headers = v8::Object::new(scope);
    for (k, v) in req.headers().iter() {
        let k = v8::String::new(scope, k.as_str()).ok_or(Error::NotAResponse)?;
        let v = v8::String::new(scope, v.to_str()?).ok_or(Error::NotAResponse)?;
        headers
            .set(scope, k.into(), v.into())
            .ok_or(Error::NotAResponse)?;
    }

    let rid = if method != Method::GET && method != Method::HEAD {
        let body = req.into_body();
        let resource = BodyResource {
            body: RefCell::new(body),
            cancel: Default::default(),
        };
        let rid = op_state.borrow_mut().resource_table.add(resource);
        v8::Integer::new_from_unsigned(scope, rid).into()
    } else {
        v8::undefined(scope).into()
    };

    let api_version = v8::String::new(scope, path.api_version()).unwrap().into();
    let path = v8::String::new(scope, path.path()).unwrap().into();
    let call_handler = service.call_handler.open(scope);
    let userid = match userid {
        Some(s) => v8::String::new(scope, s).unwrap().into(),
        None => v8::undefined(scope).into(),
    };
    let undefined = v8::undefined(scope).into();
    let result = call_handler
        .call(
            scope,
            undefined,
            &[
                userid,
                path,
                api_version,
                url.into(),
                method_value,
                headers.into(),
                rid,
            ],
        )
        .ok_or(Error::NotAResponse)?;
    Ok(v8::Global::new(scope, result))
}

async fn get_result(
    path: RequestPath,
    userid: &Option<String>,
    req: Request<hyper::Body>,
) -> Result<v8::Global<v8::Value>> {
    // Set the current path to cover JS code that runs before
    // blocking. This in particular covers code that doesn't block at
    // all.
    let result = get_result_aux(path, userid, req)?;
    // We got here without blocking and now have a future representing
    // pending work for the endpoint. resolve_promise() sets the context
    // for safe execution of request_handler; we MUST NOT block (ie,
    // `await`) between with_context() above and resolve_promise()
    // below. Otherwise, request_handler may begin executing with another,
    // wrong context.
    resolve_promise(result).await
}

async fn commit_transaction(_: ()) -> Result<Option<(Box<[u8]>, ())>, anyhow::Error> {
    // FIXME: We should always have a transaction in here
    if let Some(transaction) = take_current_transaction() {
        match crate::datastore::QueryEngine::commit_transaction_static(transaction).await {
            Ok(()) => Ok(None),
            Err(e) => {
                warn!("Commit failed: {}", e);
                Err(e)
            }
        }
    } else {
        Ok(None)
    }
}

pub(crate) async fn run_js(path: String, req: Request<hyper::Body>) -> Result<Response<Body>> {
    let qe = query_engine_arc();
    let transaction = qe.start_transaction_static().await?;
    let path = RequestPath::try_from(path.as_ref()).unwrap();
    let userid = crate::auth::get_user(&req).await?;

    with_op_state(|state| {
        set_current_transaction(state, transaction);
    });
    {
        let mut service = get();
        if service.inspector.is_some() {
            let runtime = &mut service.worker.js_runtime;
            runtime
                .inspector()
                .wait_for_session_and_break_on_next_statement();
        }
    }

    let result = get_result(path, &userid, req).await;
    // FIXME: maybe defer creating the transaction until we need one, to avoid doing it for
    // endpoints that don't do any data access. For now, because we always create it above,
    // it should be safe to unwrap.

    let transaction = take_current_transaction();

    let result = result?;

    let body = {
        // The rust borrow checker can track fields independently, but
        // only in very simple cases. For example,
        //
        //   let mut f = (1, 2);
        //   let g = &mut f.0;
        //   foo(g, f.1);
        //
        // compiles, but the following doesn't
        //
        //   let mut f = (1, 2);
        //   let g = &mut (&mut f).0;
        //   foo(g, f.1);
        //
        // The use of two service variables is to help the borrow checker
        // by accessing both fields via the same variable. In the above
        // example it would be
        //
        //   let mut f = (1, 2);
        //   let f = &mut f;
        //   let g = &mut f.0;
        //   foo(g, f.1);
        let mut service = get();
        let service: &mut DenoService = &mut service;

        let runtime = &mut service.worker.js_runtime;
        let stream = get_read_stream(runtime, result.clone())?;
        let commit_stream = try_unfold((), commit_transaction);
        let stream = stream.chain(commit_stream);

        let scope = &mut runtime.handle_scope();
        let response = result
            .open(scope)
            .to_object(scope)
            .ok_or(Error::NotAResponse)?;

        let headers: v8::Local<v8::Array> = get_member(response, scope, "headers")?;
        let num_headers = headers.length();

        let status: v8::Local<v8::Number> = get_member(response, scope, "status")?;
        let status = status.value() as u16;

        let mut builder = response_template().status(StatusCode::from_u16(status)?);

        for i in 0..num_headers {
            let value: v8::Local<v8::Array> = try_into_or(headers.get_index(scope, i))?;
            let key: v8::Local<v8::String> = try_into_or(value.get_index(scope, 0))?;
            let value: v8::Local<v8::String> = try_into_or(value.get_index(scope, 1))?;

            // FIXME: Do we have to handle non utf-8 values?
            builder = builder.header(
                key.to_rust_string_lossy(scope),
                value.to_rust_string_lossy(scope),
            );
        }

        builder.body(Body::Stream(Box::pin(stream)))?
    };

    // FIXME: We should always have a transaction in here
    if let Some(transaction) = transaction {
        with_op_state(|state| {
            set_current_transaction(state, transaction);
        });
    }
    Ok(body)
}

fn get() -> RcMut<DenoService> {
    DENO.with(|x| {
        let rc = x.get().expect("Runtime is not yet initialized.").clone();
        RcMut::new(rc)
    })
}

pub(crate) async fn compile_endpoint(path: String, code: String) -> Result<()> {
    let promise = {
        let mut service = get();
        let service: &mut DenoService = &mut service;

        let mut handle = service.module_loader.lock().unwrap();
        let code_map = &mut handle.code_map;
        let mut entry = code_map
            .entry(format!("{}.js", path))
            .and_modify(|v| v.version += 1)
            .or_insert(VersionedCode {
                code: "".to_string(),
                version: 0,
            });
        entry.code = code;

        let runtime = &mut service.worker.js_runtime;
        let scope = &mut runtime.handle_scope();
        let import_endpoint = service.import_endpoint.open(scope);
        let path = RequestPath::try_from(path.as_ref()).unwrap();
        let api_version = v8::String::new(scope, path.api_version()).unwrap().into();
        let path = v8::String::new(scope, path.path()).unwrap().into();
        let version = v8::Number::new(scope, entry.version as f64).into();
        let undefined = v8::undefined(scope).into();
        let promise = import_endpoint
            .call(scope, undefined, &[path, api_version, version])
            .unwrap();
        v8::Global::new(scope, promise)
    };
    resolve_promise(promise).await?;
    Ok(())
}

pub(crate) async fn activate_endpoint(path: &str) -> Result<()> {
    let promise = {
        let mut service = get();
        let service: &mut DenoService = &mut service;
        let runtime = &mut service.worker.js_runtime;
        let scope = &mut runtime.handle_scope();
        let activate_endpoint = service.activate_endpoint.open(scope);
        let undefined = v8::undefined(scope).into();
        let path = v8::String::new(scope, path).unwrap().into();
        let promise = activate_endpoint.call(scope, undefined, &[path]).unwrap();
        v8::Global::new(scope, promise)
    };
    resolve_promise(promise).await?;
    Ok(())
}
