// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::{response_template, Body, RequestPath};
use crate::datastore::engine::TransactionStatic;
use crate::datastore::engine::{QueryResults, ResultRow};
use crate::datastore::query::{json_to_query, Mutation};
use crate::policies::FieldPolicies;
use crate::rcmut::RcMut;
use crate::runtime;
use crate::runtime::Runtime;
use crate::types::{ObjectType, Type};
use anyhow::{anyhow, Context as AnyhowContext, Result};
use api::chisel_js;
use deno_core::error::AnyError;
use deno_core::v8;
use deno_core::CancelFuture;
use deno_core::CancelHandle;
use deno_core::JsRuntime;
use deno_core::ModuleSource;
use deno_core::ModuleSourceFuture;
use deno_core::ModuleSpecifier;
use deno_core::ModuleType;
use deno_core::OpFn;
use deno_core::OpState;
use deno_core::RcRef;
use deno_core::Resource;
use deno_core::ResourceId;
use deno_core::ZeroCopyBuf;
use deno_core::{op_async, op_sync};
use deno_fetch::reqwest;
use deno_runtime::deno_fetch;
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
use hyper::{Request, Response, StatusCode};
use log::debug;
use once_cell::unsync::OnceCell;
use pin_project::pin_project;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt::Debug;
use std::future::Future;
use std::io::Write;
use std::net::SocketAddr;
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll};
use tempfile::Builder;
use tokio::fs;

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

    module_loader: Rc<ModuleLoader>,
    handlers: HashMap<String, v8::Global<v8::Function>>,

    // Handlers that have been compiled but are not yet serving requests.
    next_handlers: HashMap<String, v8::Global<v8::Function>>,
}

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error["Endpoint didn't produce a response"]]
    NotAResponse,
}

struct ModuleLoader {
    code_map: RefCell<HashMap<String, VersionedCode>>,
    base_directory: PathBuf,
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

async fn load_code(specifier: ModuleSpecifier) -> Result<ModuleSource> {
    let mut code = if specifier.scheme() == "file" {
        fs::read_to_string(specifier.to_file_path().unwrap()).await?
    } else {
        reqwest::get(specifier.clone()).await?.text().await?
    };
    let last = specifier.path_segments().unwrap().rev().next().unwrap();
    if last.ends_with(".ts") {
        code = compile(&code, None).await?;
    }
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
            let api_path = self
                .base_directory
                .join("chisel.js")
                .to_str()
                .unwrap()
                .to_string();

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
        debug!("Deno Loading {:?}", specifier);
        load_code(specifier.clone()).boxed_local()
    }
}

fn create_web_worker(
    base_directory: PathBuf,
    bootstrap: BootstrapOptions,
    preload_module_cb: Arc<PreloadModuleCb>,
    maybe_inspector_server: Option<Arc<InspectorServer>>,
) -> Arc<CreateWebWorkerCb> {
    Arc::new(move |args| {
        let create_web_worker_cb = create_web_worker(
            base_directory.clone(),
            bootstrap.clone(),
            preload_module_cb.clone(),
            maybe_inspector_server.clone(),
        );

        let code_map = RefCell::new(HashMap::new());
        let module_loader = Rc::new(ModuleLoader {
            code_map,
            base_directory: base_directory.clone(),
        });

        // FIXME: Send a patch refactoring WebWorkerOptions and WorkerOptions
        let options = WebWorkerOptions {
            bootstrap: bootstrap.clone(),
            extensions: vec![],
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
    pub(crate) fn new(base_directory: PathBuf, inspect_brk: bool) -> Self {
        let web_worker_preload_module_cb =
            Arc::new(|worker| LocalFutureObj::new(Box::new(future::ready(Ok(worker)))));
        let code_map = RefCell::new(HashMap::new());
        let module_loader = Rc::new(ModuleLoader {
            code_map,
            base_directory: base_directory.clone(),
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
            unstable: false,
        };
        let create_web_worker_cb = create_web_worker(
            base_directory,
            bootstrap.clone(),
            web_worker_preload_module_cb.clone(),
            inspector.clone(),
        );
        let opts = WorkerOptions {
            bootstrap,
            extensions: vec![],
            unsafely_ignore_certificate_errors: None,
            root_cert_store: None,
            user_agent: "hello_runtime".to_string(),
            seed: None,
            js_error_create_fn: None,
            create_web_worker_cb,
            maybe_inspector_server: inspector.clone(),
            should_break_on_first_statement: false,
            module_loader: module_loader.clone(),
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

        let worker =
            MainWorker::bootstrap_from_options(Url::parse(path).unwrap(), permissions, opts);
        Self {
            worker,
            inspector,
            module_loader,
            handlers: HashMap::new(),
            next_handlers: HashMap::new(),
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

async fn op_chisel_read_body(
    state: Rc<RefCell<OpState>>,
    body_rid: ResourceId,
    _: (),
) -> Result<Option<ZeroCopyBuf>> {
    let resource: Rc<BodyResource> = state.borrow().resource_table.get(body_rid)?;
    let cancel = RcRef::map(&resource, |r| &r.cancel);
    let fut = ReadFuture {
        resource: resource.clone(),
    };
    let fut = fut.or_cancel(cancel);
    Ok(fut.await?.transpose()?.map(|x| x.to_vec().into()))
}

fn op_req<T1, T2, R, F, Fut>(f: F) -> Box<OpFn>
where
    T1: DeserializeOwned,
    T2: DeserializeOwned,
    R: Serialize + 'static,
    Fut: Future<Output = anyhow::Result<R>> + 'static,
    F: Fn(Rc<RefCell<OpState>>, T1, T2) -> Fut + 'static,
{
    op_async(move |s, a1, a2| {
        let inner = f(s, a1, a2);
        with_current_context(move |c| {
            let context = c.clone();
            RequestFuture { context, inner }
        })
    })
}

async fn op_chisel_store(
    _state: Rc<RefCell<OpState>>,
    content: serde_json::Value,
    _: (),
) -> Result<serde_json::Value> {
    anyhow::ensure!(
        current_method() != Method::GET,
        "Mutating the backend is not allowed during GET"
    );

    let type_name = content["name"]
        .as_str()
        .ok_or_else(|| anyhow!("Type name error; the .name key must have a string value"))?;

    let value = content["value"]
        .as_object()
        .ok_or_else(|| anyhow!("Value passed to store is not a Json Object"))?;

    let (query_engine, ty) = {
        let runtime = runtime::get();
        let api_version = current_api_version();

        // Users can only store custom types.  Builtin types are managed by us.
        let ty = match runtime
            .type_system
            .lookup_custom_type(type_name, &api_version)
        {
            Err(_) => anyhow::bail!("Cannot save into type {}.", type_name),
            Ok(ty) => ty,
        };

        let query_engine = runtime.query_engine.clone();
        (query_engine, ty)
    };

    let transaction = current_transaction()?;
    let mut transaction = transaction.lock().await;
    Ok(serde_json::json!(
        query_engine
            .add_row(&ty, value, Some(transaction.deref_mut()))
            .await?
    ))
}

async fn op_chisel_entity_delete(
    _state: Rc<RefCell<OpState>>,
    content: serde_json::Value,
    _: (),
) -> Result<serde_json::Value> {
    anyhow::ensure!(
        current_method() != Method::GET,
        "Mutating the backend is not allowed during GET"
    );

    let mutation = Mutation::parse_delete(&content).context(
        "failed to construct delete expression from JSON passed to `op_chisel_entity_delete`",
    )?;
    let query_engine = {
        let runtime = runtime::get();
        runtime.query_engine.clone()
    };
    Ok(serde_json::json!(query_engine.mutate(mutation).await?))
}

type DbStream = RefCell<QueryResults>;

/// Calculates field policies for the request being processed.
pub(crate) fn make_field_policies(runtime: &Runtime, ty: &ObjectType) -> FieldPolicies {
    let mut policies = FieldPolicies::default();
    let (path, userid) = with_current_context(|p| (p.path.path().to_string(), p.userid.clone()));
    policies.current_userid = userid;
    runtime.add_field_policies(ty, &mut policies, &path);
    policies
}

struct QueryStreamResource {
    stream: DbStream,
}

impl Resource for QueryStreamResource {}

/// Recursively builds an array representing fields of potentially nested `ty`.
/// E.g. for types X{x: Float}, and Y{a: Float, b: X} make_fields_json(Y) will
/// return [
///     ["a", "Float"],
///     [
///         {field_name": "b", "columns": ["x", "Float"]},
///         "X"
///     ]
/// ]
fn make_fields_json(ty: &Arc<ObjectType>) -> serde_json::Value {
    let mut columns = vec![];
    for f in ty.all_fields() {
        let c = match &f.type_ {
            Type::Object(nested_ty) => {
                let nested_json = serde_json::json!({
                    "field_name": f.name.to_owned(),
                    "columns": make_fields_json(nested_ty)
                });
                serde_json::json!(vec![nested_json, serde_json::json!(f.type_.name())])
            }
            _ => serde_json::json!(vec![f.name.to_owned(), f.type_.name().to_string()]),
        };
        columns.push(c);
    }
    serde_json::json!(columns)
}

fn op_chisel_introspect(
    _op_state: &mut OpState,
    value: serde_json::Value,
    _: (),
) -> Result<serde_json::Value> {
    let runtime = runtime::get();
    let api_version = current_api_version();

    let type_name = value["name"]
        .as_str()
        .ok_or_else(|| anyhow!("expecting to be asked for a name"))?;

    // Could be the OAuthUser, so have to lookup builtins as well
    let ty = match runtime
        .type_system
        .lookup_custom_type(type_name, &api_version)
    {
        Ok(ty) => ty,
        Err(_) => match runtime.type_system.lookup_builtin_type(type_name)? {
            Type::Object(ty) => ty,
            _ => anyhow::bail!("Invalid to introspect {}", type_name),
        },
    };
    Ok(make_fields_json(&ty))
}

fn op_chisel_get_secret(
    _op_state: &mut OpState,
    secret: serde_json::Value,
    _: (),
) -> Result<Option<serde_json::Value>> {
    let key = secret
        .as_str()
        .ok_or_else(|| anyhow!("secret key is not a string"))?;
    let runtime = runtime::get();
    Ok(runtime.secrets.get_secret(key))
}

fn op_chisel_relational_query_create(
    op_state: &mut OpState,
    relation: serde_json::Value,
    _: (),
) -> Result<ResourceId> {
    // FIXME: It is silly do create a serde_json::Value just to
    // convert it to something else. The difficulty with decoding
    // directly is that we need to implement visit_map to read the
    // kind field to see what we should deserialize. We can only look
    // once at each K,V pair, so we have to keep the V as
    // serde_v8::value, which means we need a scope to then
    // deserialize those. There is a scope is the decoder, but there
    // is no way to access it from here. We would have to replace
    // op_chisel_relational_query_create with a closure that has an
    // Rc<DenoService>.
    let query = json_to_query(&relation)?;
    let mut runtime = runtime::get();
    let query_engine = &mut runtime.query_engine;

    let transaction = current_transaction()?;
    let stream = query_engine.query(transaction, query)?;
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

async fn op_chisel_relational_query_next(
    state: Rc<RefCell<OpState>>,
    query_stream_rid: ResourceId,
    _: (),
) -> Result<Option<ResultRow>> {
    let resource: Rc<QueryStreamResource> = state.borrow().resource_table.get(query_stream_rid)?;
    let fut = QueryNextFuture { resource };
    if let Some(row) = fut.await {
        Ok(Some(row?))
    } else {
        Ok(None)
    }
}

async fn op_chisel_user(_: Rc<RefCell<OpState>>, _: (), _: ()) -> Result<serde_json::Value> {
    match with_current_context(|path| path.userid.clone()) {
        None => Ok(serde_json::Value::Null),
        Some(id) => Ok(serde_json::Value::String(id)),
    }
}

// Used by deno to format names in errors
fn op_format_file_name(_: &mut OpState, file_name: String, _: ()) -> Result<String> {
    Ok(file_name)
}

async fn create_deno<P: AsRef<Path>>(base_directory: P, inspect_brk: bool) -> Result<DenoService> {
    let mut d = DenoService::new(base_directory.as_ref().to_owned(), inspect_brk);
    let worker = &mut d.worker;
    let runtime = &mut worker.js_runtime;

    // FIXME: Turn this into a deno extension
    runtime.register_op("op_format_file_name", op_sync(op_format_file_name));
    runtime.register_op("chisel_read_body", op_req(op_chisel_read_body));
    runtime.register_op("chisel_store", op_req(op_chisel_store));
    runtime.register_op("chisel_entity_delete", op_req(op_chisel_entity_delete));
    runtime.register_op("chisel_get_secret", op_sync(op_chisel_get_secret));
    runtime.register_op(
        "chisel_relational_query_create",
        op_sync(op_chisel_relational_query_create),
    );
    runtime.register_op(
        "chisel_relational_query_next",
        op_req(op_chisel_relational_query_next),
    );
    runtime.register_op("chisel_introspect", op_sync(op_chisel_introspect));
    runtime.register_op("chisel_user", op_req(op_chisel_user));
    runtime.sync_ops_cache();

    // FIXME: Include these files in the snapshop

    let chisel = chisel_js().to_string();
    let chisel_path = base_directory.as_ref().join("chisel.js");
    fs::write(&chisel_path, &chisel).await?;
    let chisel_path = chisel_path.to_str().unwrap().to_string();

    let main = "import * as Chisel from \"./chisel.js\";
                       globalThis.Chisel = Chisel;"
        .to_string();
    let main_path = base_directory.as_ref().join("main.js");
    fs::write(&main_path, &main).await?;
    let main_path = main_path.to_str().unwrap().to_string();

    {
        let mut code_map = d.module_loader.code_map.borrow_mut();
        code_map.insert(
            main_path.clone(),
            VersionedCode {
                code: main,
                version: 0,
            },
        );

        code_map.insert(
            chisel_path.clone(),
            VersionedCode {
                code: chisel,
                version: 0,
            },
        );
    }

    worker
        .execute_main_module(&ModuleSpecifier::parse(&format!("file://{}", &main_path)).unwrap())
        .await?;
    Ok(d)
}

pub(crate) async fn init_deno<P: AsRef<Path>>(base_directory: P, inspect_brk: bool) -> Result<()> {
    let service = Rc::new(RefCell::new(
        create_deno(base_directory, inspect_brk).await?,
    ));
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
    context: RequestContext,
    js_promise: v8::Global<v8::Value>,
) -> impl Future<Output = Result<v8::Global<v8::Value>>> {
    let inner = ResolveFuture { js_promise };
    RequestFuture { context, inner }
}

async fn get_read_future(
    read_tpl: Option<(
        v8::Global<v8::Value>,
        v8::Global<v8::Function>,
        RequestContext,
    )>,
) -> Result<Option<(Box<[u8]>, ())>> {
    let (reader, read, context) = match read_tpl {
        Some(x) => x,
        None => {
            return Ok(None);
        }
    };

    let js_promise = {
        let mut service = get();
        let runtime = &mut service.worker.js_runtime;
        let scope = &mut runtime.handle_scope();
        let reader = v8::Local::new(scope, reader.clone());
        let res = read
            .open(scope)
            .call(scope, reader, &[])
            .ok_or(Error::NotAResponse)?;
        v8::Global::new(scope, res)
    };
    let read_result = resolve_promise(context, js_promise).await?;
    let mut service = get();
    let runtime = &mut service.worker.js_runtime;
    let scope = &mut runtime.handle_scope();
    let read_result = read_result
        .open(scope)
        .to_object(scope)
        .ok_or(Error::NotAResponse)?;
    let done: v8::Local<v8::Boolean> = get_member(read_result, scope, "done")?;
    if done.is_true() {
        return Ok(None);
    }
    let value: v8::Local<v8::ArrayBufferView> = get_member(read_result, scope, "value")?;
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
    context: RequestContext,
) -> Result<impl Stream<Item = Result<Box<[u8]>>>> {
    let scope = &mut runtime.handle_scope();
    let response = global_response
        .open(scope)
        .to_object(scope)
        .ok_or(Error::NotAResponse)?;

    let read = match get_member::<v8::Local<v8::Object>>(response, scope, "body") {
        Ok(body) => {
            let get_reader: v8::Local<v8::Function> = get_member(body, scope, "getReader")?;
            let reader: v8::Local<v8::Object> =
                try_into_or(get_reader.call(scope, body.into(), &[]))?;
            let read: v8::Local<v8::Function> = get_member(reader, scope, "read")?;
            let reader: v8::Local<v8::Value> = reader.into();
            let reader: v8::Global<v8::Value> = v8::Global::new(scope, reader);
            let read = v8::Global::new(scope, read);
            Some((reader, read, context))
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

#[derive(Default, Clone)]
struct RequestContext {
    path: RequestPath,
    method: Method,
    /// Uniquely identifies the OAuthUser row for the logged-in user.  None if there was no login.
    userid: Option<String>,
    transaction: Option<TransactionStatic>,
}

mod context {
    use crate::deno::RequestContext;
    use std::cell::RefCell;
    thread_local! {
        static CURRENT_CONTEXT : RefCell<Option<RequestContext>> = RefCell::new(None);
    }
    pub(super) fn with_current_context<F, R>(f: F) -> R
    where
        F: FnOnce(&RequestContext) -> R,
    {
        CURRENT_CONTEXT.with(|cx| f(cx.borrow().as_ref().unwrap()))
    }

    pub(super) fn with_context<F, R>(nc: RequestContext, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        CURRENT_CONTEXT.with(|cx| {
            let old = cx.borrow().clone();
            cx.replace(Some(nc));
            let ret = f();
            cx.replace(old);
            ret
        })
    }
}
use context::with_context;
use context::with_current_context;

pub(crate) fn current_api_version() -> String {
    with_current_context(|p| p.path.api_version().to_string())
}

fn current_method() -> Method {
    with_current_context(|path| path.method.clone())
}

fn current_transaction() -> Result<TransactionStatic> {
    with_current_context(|path| {
        path.transaction
            .clone()
            .ok_or_else(|| anyhow!("no active transaction"))
    })
}

// This is a wrapper future that sets the context before polling. This
// is necessary, since future execution can interleave steps from
// different requests.
#[pin_project]
struct RequestFuture<F: Future> {
    context: RequestContext,
    #[pin]
    inner: F,
}

impl<F: Future> Future for RequestFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, c: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        with_context(this.context.clone(), || this.inner.poll(c))
    }
}

fn get_result_aux(
    request_handler: v8::Global<v8::Function>,
    req: &mut Request<hyper::Body>,
) -> Result<v8::Global<v8::Value>> {
    let mut service = get();
    let runtime = &mut service.worker.js_runtime;

    let op_state = runtime.op_state();
    let global_context = runtime.global_context();
    let scope = &mut runtime.handle_scope();
    let global_proxy = global_context.open(scope).global(scope);

    // FIXME: this request conversion is probably simplistic. Check deno/ext/http/lib.rs
    let request: v8::Local<v8::Function> = get_member(global_proxy, scope, "Request")?;
    let url = v8::String::new(scope, &req.uri().to_string()).unwrap();
    let init = v8::Object::new(scope);

    let method = req.method();
    let method_key = v8::String::new(scope, "method").unwrap().into();
    let method_value = v8::String::new(scope, method.as_str()).unwrap().into();
    init.set(scope, method_key, method_value)
        .ok_or(Error::NotAResponse)?;

    let headers = v8::Object::new(scope);
    for (k, v) in req.headers().iter() {
        let k = v8::String::new(scope, k.as_str()).ok_or(Error::NotAResponse)?;
        let v = v8::String::new(scope, v.to_str()?).ok_or(Error::NotAResponse)?;
        headers
            .set(scope, k.into(), v.into())
            .ok_or(Error::NotAResponse)?;
    }
    let headers_key = v8::String::new(scope, "headers").unwrap().into();
    init.set(scope, headers_key, headers.into())
        .ok_or(Error::NotAResponse)?;

    if method != Method::GET && method != Method::HEAD {
        let body = hyper::Body::empty();
        let body = std::mem::replace(req.body_mut(), body);
        let resource = BodyResource {
            body: RefCell::new(body),
            cancel: Default::default(),
        };
        let rid = op_state.borrow_mut().resource_table.add(resource);
        let rid = v8::Integer::new_from_unsigned(scope, rid).into();

        let chisel: v8::Local<v8::Object> = get_member(global_proxy, scope, "Chisel")?;
        let build: v8::Local<v8::Function> =
            get_member(chisel, scope, "buildReadableStreamForBody")?;
        let body = build.call(scope, chisel.into(), &[rid]).unwrap();
        let body_key = v8::String::new(scope, "body")
            .ok_or(Error::NotAResponse)?
            .into();
        init.set(scope, body_key, body).ok_or(Error::NotAResponse)?;
    }

    let request = request
        .new_instance(scope, &[url.into(), init.into()])
        .ok_or(Error::NotAResponse)?;

    let result = request_handler
        .open(scope)
        .call(scope, global_proxy.into(), &[request.into()])
        .ok_or(Error::NotAResponse)?;
    Ok(v8::Global::new(scope, result))
}

async fn get_result(
    request_handler: v8::Global<v8::Function>,
    req: &mut Request<hyper::Body>,
    context: RequestContext,
) -> Result<v8::Global<v8::Value>> {
    // Set the current path to cover JS code that runs before
    // blocking. This in particular covers code that doesn't block at
    // all.
    let result = with_context(context.clone(), || get_result_aux(request_handler, req))?;
    // We got here without blocking and now have a future representing
    // pending work for the endpoint. The future returned by
    // resolve_promise takes care of setting the context.
    resolve_promise(context, result).await
}

async fn commit_transaction(
    transaction: TransactionStatic,
) -> Result<Option<(Box<[u8]>, TransactionStatic)>, anyhow::Error> {
    match crate::datastore::QueryEngine::commit_transaction_static(transaction).await {
        Ok(()) => Ok(None),
        Err(e) => {
            warn!("Commit failed: {}", e);
            Err(e)
        }
    }
}

pub(crate) async fn run_js(path: String, mut req: Request<hyper::Body>) -> Result<Response<Body>> {
    let qe = {
        let runtime = runtime::get();
        let qe = runtime.query_engine.clone();
        drop(runtime);
        qe
    };

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
    let request_handler = {
        let mut service = get();
        let service: &mut DenoService = &mut service;

        let request_handler = service.handlers.get(&path).unwrap().clone();
        let runtime = &mut service.worker.js_runtime;

        if service.inspector.is_some() {
            runtime
                .inspector()
                .wait_for_session_and_break_on_next_statement();
        }
        request_handler
    };

    let transaction = qe.start_transaction_static().await?;
    let context = RequestContext {
        method: req.method().clone(),
        path: RequestPath::try_from(path.as_ref()).unwrap(),
        userid: crate::auth::get_user(&req).await?,
        transaction: Some(transaction.clone()),
    };
    let result = get_result(request_handler, &mut req, context.clone()).await;
    // FIXME: maybe defer creating the transaction until we need one, to avoid doing it for
    // endpoints that don't do any data access. For now, because we always create it above,
    // it should be safe to unwrap.
    let result = result?;

    let body = {
        let mut service = get();
        let service: &mut DenoService = &mut service;
        let runtime = &mut service.worker.js_runtime;

        let stream = get_read_stream(runtime, result.clone(), context)?;
        let commit_stream = try_unfold(transaction.clone(), commit_transaction);
        let stream = stream.chain(commit_stream);

        let scope = &mut runtime.handle_scope();
        let response = result
            .open(scope)
            .to_object(scope)
            .ok_or(Error::NotAResponse)?;

        let status: v8::Local<v8::Number> = get_member(response, scope, "status")?;
        let status = status.value() as u16;

        let headers: v8::Local<v8::Object> = get_member(response, scope, "headers")?;
        let entries: v8::Local<v8::Function> = get_member(headers, scope, "entries")?;
        let iter: v8::Local<v8::Object> = try_into_or(entries.call(scope, headers.into(), &[]))?;

        let next: v8::Local<v8::Function> = get_member(iter, scope, "next")?;
        let mut builder = response_template().status(StatusCode::from_u16(status)?);

        loop {
            let item: v8::Local<v8::Object> = try_into_or(next.call(scope, iter.into(), &[]))?;

            let done: v8::Local<v8::Value> = get_member(item, scope, "done")?;
            if done.is_true() {
                break;
            }
            let value: v8::Local<v8::Array> = get_member(item, scope, "value")?;
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
    Ok(body)
}

fn get() -> RcMut<DenoService> {
    DENO.with(|x| {
        let rc = x.get().expect("Runtime is not yet initialized.").clone();
        RcMut::new(rc)
    })
}

pub(crate) async fn compile_endpoint<P: AsRef<Path>>(
    base_directory: P,
    path: String,
    code: String,
) -> Result<()> {
    let promise = {
        let mut service = get();
        let service: &mut DenoService = &mut service;

        let mut code_map = service.module_loader.code_map.borrow_mut();
        let mut entry = code_map
            .entry(path.clone())
            .and_modify(|v| v.version += 1)
            .or_insert(VersionedCode {
                code: "".to_string(),
                version: 0,
            });
        entry.code = code;

        // Modules are never unloaded, so we need to create an unique
        // path. This will not be a problem once we publish the entire app
        // at once, since then we can create a new isolate for it.
        let url = format!(
            "file://{}/{}.js?ver={}",
            base_directory.as_ref().display(),
            path,
            entry.version
        );
        let url = Url::parse(&url).unwrap();

        drop(code_map);
        let runtime = &mut service.worker.js_runtime;
        runtime.execute_script(&path, &format!("import(\"{}\")", url))?
    };

    let context = RequestContext {
        method: Method::GET,
        path: RequestPath::try_from(path.as_ref()).unwrap(),
        userid: None,
        transaction: None,
    };
    let module = resolve_promise(context, promise).await?;

    let mut service = get();
    let service: &mut DenoService = &mut service;
    let runtime = &mut service.worker.js_runtime;
    let scope = &mut runtime.handle_scope();
    let module = module
        .open(scope)
        .to_object(scope)
        .ok_or(Error::NotAResponse)?;
    let request_handler: v8::Local<v8::Function> = get_member(module, scope, "default")?;
    service
        .next_handlers
        .insert(path, v8::Global::new(scope, request_handler));
    Ok(())
}

pub(crate) fn activate_endpoint(path: &str) {
    let mut service = get();
    let (path, handler) = service.next_handlers.remove_entry(path).unwrap();
    service.handlers.insert(path, handler);
}
