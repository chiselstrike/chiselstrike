// SPDX-FileCopyrightText: © 2021-2022 ChiselStrike <info@chiselstrike.com>

use crate::api::{Body, RequestPath};
use crate::db::convert;
use crate::policies::FieldPolicies;
use crate::query::engine::JsonObject;
use crate::query::engine::SqlStream;
use crate::rcmut::RcMut;
use crate::runtime;
use crate::runtime::Runtime;
use crate::types::{ObjectType, Type};
use anyhow::{anyhow, Result};
use deno_broadcast_channel::InMemoryBroadcastChannel;
use deno_core::error::AnyError;
use deno_core::CancelFuture;
use deno_core::CancelHandle;
use deno_core::JsRuntime;
use deno_core::ModuleSource;
use deno_core::ModuleSourceFuture;
use deno_core::ModuleSpecifier;
use deno_core::OpState;
use deno_core::RcRef;
use deno_core::Resource;
use deno_core::ResourceId;
use deno_core::ZeroCopyBuf;
use deno_core::{op_async, op_sync};
use deno_runtime::inspector_server::InspectorServer;
use deno_runtime::permissions::Permissions;
use deno_runtime::worker::{MainWorker, WorkerOptions};
use deno_runtime::BootstrapOptions;
use deno_web::BlobStore;
use futures::stream::{try_unfold, Stream};
use futures::FutureExt;
use hyper::body::HttpBody;
use hyper::header::HeaderValue;
use hyper::Method;
use hyper::{Request, Response, StatusCode};
use log::debug;
use once_cell::unsync::OnceCell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryInto;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::fs;

// FIXME: This should not be here. The client should download and
// compile modules, the server should not get code out of the
// internet.
use compile::compile_ts_code;

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
        module_url_specified: specifier.to_string(),
        module_url_found: specifier.to_string(),
    })
}

async fn load_code(specifier: ModuleSpecifier) -> Result<ModuleSource> {
    let code = if specifier.scheme() == "file" {
        fs::read_to_string(specifier.to_file_path().unwrap()).await?
    } else {
        reqwest::get(specifier.clone()).await?.text().await?
    };
    let code = compile_ts_code(code)?;
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
        if specifier == "@chiselstrike/chiselstrike" {
            let api_path = self
                .base_directory
                .join("chisel.ts")
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

impl DenoService {
    pub(crate) fn new(base_directory: PathBuf, inspect_brk: bool) -> Self {
        let create_web_worker_cb = Arc::new(|_| {
            todo!("Web workers are not supported");
        });
        let code_map = RefCell::new(HashMap::new());
        let module_loader = Rc::new(ModuleLoader {
            code_map,
            base_directory,
        });

        let mut inspector = None;
        if inspect_brk {
            let addr: SocketAddr = "127.0.0.1:9229".parse().unwrap();
            inspector = Some(Arc::new(InspectorServer::new(addr, "chisel".to_string())));
        }

        let opts = WorkerOptions {
            bootstrap: BootstrapOptions {
                apply_source_maps: false,
                args: vec![],
                cpu_count: 1,
                debug_flag: false,
                enable_testing_features: false,
                // FIXME: make location a configuration parameter
                location: Some(Url::parse("http://chiselstrike.com").unwrap()),
                no_color: true,
                runtime_version: "x".to_string(),
                ts_version: "x".to_string(),
                unstable: false,
            },
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
            blob_store: BlobStore::default(),
            broadcast_channel: InMemoryBroadcastChannel::default(),
            shared_array_buffer_store: None,
            compiled_wasm_module_store: None,
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

async fn op_chisel_read_body(
    state: Rc<RefCell<OpState>>,
    body_rid: ResourceId,
    _: (),
) -> Result<Option<ZeroCopyBuf>> {
    let resource: Rc<BodyResource> = state.borrow().resource_table.get(body_rid)?;
    let cancel = RcRef::map(&resource, |r| &r.cancel);
    let mut borrow = resource.body.borrow_mut();
    let fut = borrow.data().or_cancel(cancel);
    Ok(fut.await?.transpose()?.map(|x| x.to_vec().into()))
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
    // Await point below, RcMut can't be held.
    drop(runtime);

    Ok(serde_json::json!(query_engine.add_row(&ty, value).await?))
}

type DbStream = RefCell<SqlStream>;

pub(crate) fn get_policies(runtime: &Runtime, ty: &ObjectType) -> anyhow::Result<FieldPolicies> {
    let mut policies = FieldPolicies::default();
    CURRENT_CONTEXT.with(|p| runtime.get_policies(ty, &mut policies, p.borrow().path.path()));
    Ok(policies)
}

struct QueryStreamResource {
    stream: DbStream,
}

impl Resource for QueryStreamResource {}

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

    let vec: Vec<serde_json::Value> = ty
        .all_fields()
        .map(|f| serde_json::json!(vec![f.name.clone(), f.type_.name().to_string()]))
        .collect();
    Ok(serde_json::json!(vec))
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
    let relation = convert(&relation)?;
    let mut runtime = runtime::get();
    let query_engine = &mut runtime.query_engine;
    let stream = Box::pin(query_engine.query_relation(relation));
    let resource = QueryStreamResource {
        stream: RefCell::new(stream),
    };
    let rid = op_state.resource_table.add(resource);
    Ok(rid)
}

async fn op_chisel_relational_query_next(
    state: Rc<RefCell<OpState>>,
    query_stream_rid: ResourceId,
    _: (),
) -> Result<Option<JsonObject>> {
    let resource: Rc<QueryStreamResource> = state.borrow().resource_table.get(query_stream_rid)?;
    let mut stream = resource.stream.borrow_mut();
    use futures::stream::StreamExt;

    if let Some(row) = stream.next().await {
        Ok(Some(row?))
    } else {
        Ok(None)
    }
}

fn compile_ts_code_as_bytes(code: &[u8]) -> Result<String> {
    let code = std::str::from_utf8(code)?.to_string();
    compile_ts_code(code)
}

async fn create_deno<P: AsRef<Path>>(base_directory: P, inspect_brk: bool) -> Result<DenoService> {
    let mut d = DenoService::new(base_directory.as_ref().to_owned(), inspect_brk);
    let worker = &mut d.worker;
    let runtime = &mut worker.js_runtime;

    // FIXME: Turn this into a deno extension
    runtime.register_op("chisel_read_body", op_async(op_chisel_read_body));
    runtime.register_op("chisel_store", op_async(op_chisel_store));
    runtime.register_op(
        "chisel_relational_query_create",
        op_sync(op_chisel_relational_query_create),
    );
    runtime.register_op(
        "chisel_relational_query_next",
        op_async(op_chisel_relational_query_next),
    );
    runtime.register_op("chisel_introspect", op_sync(op_chisel_introspect));
    runtime.sync_ops_cache();

    // FIXME: Include these files in the snapshop
    let chisel = compile_ts_code_as_bytes(include_bytes!("chisel.ts"))?;
    let chisel_path = base_directory
        .as_ref()
        .join("chisel.ts")
        .to_str()
        .unwrap()
        .to_string();
    {
        let mut code_map = d.module_loader.code_map.borrow_mut();
        code_map.insert(
            chisel_path.clone(),
            VersionedCode {
                code: chisel,
                version: 0,
            },
        );
    }

    worker
        .execute_main_module(&ModuleSpecifier::parse(&format!("file://{}", &chisel_path)).unwrap())
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

async fn get_read_future(
    reader: v8::Global<v8::Value>,
    read: v8::Global<v8::Function>,
) -> Result<Option<(Box<[u8]>, ())>> {
    let mut service = get();
    let runtime = &mut service.worker.js_runtime;
    let js_promise = {
        let scope = &mut runtime.handle_scope();
        let reader = v8::Local::new(scope, reader.clone());
        let res = read
            .open(scope)
            .call(scope, reader, &[])
            .ok_or(Error::NotAResponse)?;
        v8::Global::new(scope, res)
    };
    let read_result = runtime.resolve_value(js_promise).await?;
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
) -> Result<impl Stream<Item = Result<Box<[u8]>>>> {
    let scope = &mut runtime.handle_scope();
    let response = global_response
        .open(scope)
        .to_object(scope)
        .ok_or(Error::NotAResponse)?;

    let body: v8::Local<v8::Object> = get_member(response, scope, "body")?;
    let get_reader: v8::Local<v8::Function> = get_member(body, scope, "getReader")?;
    let reader: v8::Local<v8::Object> = try_into_or(get_reader.call(scope, body.into(), &[]))?;
    let read: v8::Local<v8::Function> = get_member(reader, scope, "read")?;
    let reader: v8::Local<v8::Value> = reader.into();
    let reader: v8::Global<v8::Value> = v8::Global::new(scope, reader);
    let read = v8::Global::new(scope, read);

    let stream = try_unfold((), move |_| get_read_future(reader.clone(), read.clone()));
    Ok(stream)
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

#[derive(Default)]
struct RequestContext {
    path: RequestPath,
    method: Method,
}

thread_local! {
    static CURRENT_CONTEXT : RefCell<RequestContext> = RefCell::new(Default::default());
}

pub(crate) fn current_api_version() -> String {
    CURRENT_CONTEXT.with(|p| {
        let x = p.borrow();
        x.path.api_version().to_string()
    })
}

fn set_current_context(current_path: String, method: Method) {
    let rp = RequestPath::try_from(current_path.as_ref()).unwrap();

    CURRENT_CONTEXT.with(|path| {
        let mut borrow = path.borrow_mut();
        borrow.path = rp;
        borrow.method = method;
    });
}

fn current_method() -> Method {
    CURRENT_CONTEXT.with(|path| path.borrow().method.clone())
}

struct RequestFuture<F> {
    request_path: String,
    request_method: Method,
    inner: F,
}

impl<F: Future> Future for RequestFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, c: &mut Context<'_>) -> Poll<F::Output> {
        set_current_context(self.request_path.clone(), self.request_method.clone());
        // Structural Pinning, it is OK because inner is pinned when we are.
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        inner.poll(c)
    }
}

fn get_result_aux(
    runtime: &mut JsRuntime,
    request_handler: v8::Global<v8::Function>,
    req: &mut Request<hyper::Body>,
) -> Result<v8::Global<v8::Value>> {
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
    runtime: &mut JsRuntime,
    request_handler: v8::Global<v8::Function>,
    req: &mut Request<hyper::Body>,
    path: String,
) -> Result<v8::Global<v8::Value>> {
    let method = req.method().clone();
    // Set the current path to cover JS code that runs before
    // blocking. This in particular covers code that doesn't block at
    // all.
    set_current_context(path.clone(), method.clone());
    let result = get_result_aux(runtime, request_handler, req)?;
    let result = runtime.resolve_value(result);
    // We got here without blocking and now have a future representing
    // pending work for the endpoint. We might not get to that future
    // before the current path is changed, so wrap the future in a
    // RequestFuture that will reset the current path before polling.
    RequestFuture {
        request_path: path,
        request_method: method,
        inner: result,
    }
    .await
}

pub(crate) async fn run_js(path: String, mut req: Request<hyper::Body>) -> Result<Response<Body>> {
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

    let request_handler = service.handlers.get(&path).unwrap().clone();
    let runtime = &mut service.worker.js_runtime;

    if service.inspector.is_some() {
        runtime
            .inspector()
            .wait_for_session_and_break_on_next_statement();
    }

    let result = get_result(runtime, request_handler, &mut req, path).await?;

    let stream = get_read_stream(runtime, result.clone())?;
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
    let mut builder = Response::builder().status(StatusCode::from_u16(status)?);

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

    let headers = builder.headers_mut().ok_or(Error::NotAResponse)?;
    let entry = headers.entry("Access-Control-Allow-Origin");
    entry.or_insert(HeaderValue::from_static("*"));
    let entry = headers.entry("Access-Control-Allow-Methods");
    entry.or_insert(HeaderValue::from_static("POST, PUT, GET, OPTIONS"));
    let entry = headers.entry("Access-Control-Allow-Headers");
    entry.or_insert(HeaderValue::from_static("Content-Type"));

    let body = builder.body(Body::Stream(Box::pin(stream)))?;
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
        "file://{}/{}.ts?ver={}",
        base_directory.as_ref().display(),
        path,
        entry.version
    );
    let url = Url::parse(&url).unwrap();

    drop(code_map);
    let runtime = &mut service.worker.js_runtime;
    let promise = runtime.execute_script(&path, &format!("import(\"{}\")", url))?;
    let module = runtime.resolve_value(promise).await?;
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

pub(crate) fn define_type(ty: &ObjectType) -> Result<()> {
    let mut service = get();
    let runtime = &mut service.worker.js_runtime;
    let global_context = runtime.global_context();
    let scope = &mut runtime.handle_scope();
    let global_proxy = global_context.open(scope).global(scope);
    let chisel: v8::Local<v8::Object> = get_member(global_proxy, scope, "Chisel").unwrap();
    let api: v8::Local<v8::Object> = get_member(chisel, scope, "api")?;
    let chisel_func: v8::Local<v8::Function> = get_member(api, scope, "chiselIterator")?;

    let mut fields = vec![];
    for f in ty.all_fields() {
        let name = v8::String::new(scope, &f.name).unwrap().into();
        let ty_name = f.type_.name();
        let ty_name = v8::String::new(scope, ty_name).unwrap().into();
        let tuple = v8::Array::new_with_elements(scope, &[name, ty_name]).into();
        fields.push(tuple);
    }

    let columns = v8::Array::new_with_elements(scope, &fields).into();
    let name = v8::String::new(scope, ty.name()).unwrap();
    let chisel_func = try_into_or(chisel_func.call(scope, api.into(), &[name.into(), columns]))?;

    chisel.set(scope, name.into(), chisel_func).unwrap();
    Ok(())
}

pub(crate) fn flush_types() -> Result<()> {
    let mut service = get();
    let runtime = &mut service.worker.js_runtime;
    let global_context = runtime.global_context();
    let scope = &mut runtime.handle_scope();
    let global_proxy = global_context.open(scope).global(scope);
    let chisel: v8::Local<v8::Object> = get_member(global_proxy, scope, "Chisel").unwrap();

    let collections = v8::String::new(scope, "collections").unwrap().into();
    let empty = v8::Object::new(scope);
    chisel.set(scope, collections, empty.into());
    Ok(())
}
