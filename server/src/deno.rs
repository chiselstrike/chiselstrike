// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::{Body, RequestPath};
use crate::db::convert;
use crate::policies::FieldPolicies;
use crate::query::engine::JsonObject;
use crate::query::engine::SqlStream;
use crate::rcmut::RcMut;
use crate::runtime;
use crate::runtime::Runtime;
use crate::types::ObjectType;
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
use once_cell::unsync::OnceCell;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::convert::TryInto;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll};
use swc_common::sync::Lrc;
use swc_common::{
    errors::{emitter, Handler},
    source_map::FileName,
    SourceMap,
};
use swc_ecma_codegen::{text_writer::JsWriter, Emitter};
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax};
use swc_ecma_visit::FoldWith;

use url::Url;

struct VersionedHandler {
    // This is None when there is a failure loading the module. In
    // that case we still need the version to be set so that it is
    // possible to change the endpoint.
    func: Option<v8::Global<v8::Function>>,
    version: u64,
}

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
    handlers: HashMap<String, VersionedHandler>,
}

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error["Endpoint didn't produce a response"]]
    NotAResponse,
}

struct ModuleLoader {
    code_map: RefCell<HashMap<String, String>>,
}

const DUMMY_PREFIX: &str = "file://$chisel$";

fn wrap(specifier: &ModuleSpecifier, code: String) -> Result<ModuleSource> {
    let code = compile_ts_code(code)?;
    Ok(ModuleSource {
        code,
        module_url_specified: specifier.to_string(),
        module_url_found: specifier.to_string(),
    })
}

async fn load_code(specifier: ModuleSpecifier) -> Result<ModuleSource> {
    let code = reqwest::get(specifier.clone()).await?.text().await?;
    wrap(&specifier, code)
}

impl deno_core::ModuleLoader for ModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _is_main: bool,
    ) -> Result<ModuleSpecifier, AnyError> {
        Ok(deno_core::resolve_import(specifier, referrer)?)
    }

    fn load(
        &self,
        specifier: &ModuleSpecifier,
        _maybe_referrer: Option<ModuleSpecifier>,
        _is_dyn_import: bool,
    ) -> Pin<Box<ModuleSourceFuture>> {
        if specifier.as_str().starts_with(DUMMY_PREFIX) {
            let path = specifier.path();
            let code = self.code_map.borrow().get(path).unwrap().clone();
            let code = wrap(specifier, code);
            std::future::ready(code).boxed_local()
        } else {
            load_code(specifier.clone()).boxed_local()
        }
    }
}

impl DenoService {
    pub(crate) fn new(inspect_brk: bool) -> Self {
        let create_web_worker_cb = Arc::new(|_| {
            todo!("Web workers are not supported");
        });
        let code_map = RefCell::new(HashMap::new());
        let module_loader = Rc::new(ModuleLoader { code_map });

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
) -> Result<()> {
    let type_name = content["name"]
        .as_str()
        .ok_or_else(|| anyhow!("Type name error; the .name key must have a string value"))?;
    let runtime = runtime::get();
    let api_version = current_api_version();

    let ty = runtime
        .type_system
        .lookup_object_type(type_name, &api_version)?;

    runtime.query_engine.add_row(&ty, &content["value"]).await
}

type DbStream = RefCell<SqlStream>;

pub(crate) fn get_policies(runtime: &Runtime, ty: &ObjectType) -> anyhow::Result<FieldPolicies> {
    let mut policies = FieldPolicies::default();
    CURRENT_REQUEST_PATH.with(|p| runtime.get_policies(ty, &mut policies, p.borrow().path()));
    Ok(policies)
}

struct QueryStreamResource {
    stream: DbStream,
}

impl Resource for QueryStreamResource {}

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

// FIXME: This should not be here. The client should download and
// compile modules, the server should not get code out of the
// internet.
// FIXME: This should produce an error when failing to compile.
fn compile_ts_code(code: String) -> Result<String> {
    #[derive(Clone)]
    struct ErrorBuffer {
        inner: Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl ErrorBuffer {
        fn new() -> Self {
            Self {
                inner: Arc::new(std::sync::Mutex::new(vec![])),
            }
        }

        fn get(&self) -> String {
            String::from_utf8_lossy(&self.inner.lock().unwrap().clone()).to_string()
        }
    }

    impl std::io::Write for ErrorBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut v = self.inner.lock().unwrap();
            v.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let err_buf = ErrorBuffer::new();

    let cm: Lrc<SourceMap> = Default::default();
    let emitter = Box::new(emitter::EmitterWriter::new(
        Box::new(err_buf.clone()),
        Some(cm.clone()),
        false,
        true,
    ));
    let handler = Handler::with_emitter(true, false, emitter);

    // FIXME: We probably need a name for better error messages.
    let fm = cm.new_source_file(FileName::Anon, code);
    let lexer = Lexer::new(
        Syntax::Typescript(Default::default()),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );

    let mut parser = Parser::new_from(lexer);

    for e in parser.take_errors() {
        e.into_diagnostic(&handler).emit();
    }

    let module = parser.parse_typescript_module().map_err(|e| {
        // Unrecoverable fatal error occurred
        e.into_diagnostic(&handler).emit();
        anyhow!("Parse failed: {}", err_buf.get())
    })?;

    // Remove typescript types
    let module = module.fold_with(&mut swc_ecma_transforms_typescript::strip());

    let mut buf = vec![];
    {
        let mut emitter = Emitter {
            cfg: swc_ecma_codegen::Config {
                ..Default::default()
            },
            cm: cm.clone(),
            comments: None,
            wr: JsWriter::new(cm, "\n", &mut buf, None),
        };
        emitter.emit_module(&module).unwrap();
    }
    Ok(String::from_utf8_lossy(&buf).to_string())
}

fn compile_ts_code_as_bytes(code: &[u8]) -> Result<String> {
    let code = std::str::from_utf8(code)?.to_string();
    compile_ts_code(code)
}

async fn create_deno(inspect_brk: bool) -> Result<DenoService> {
    let mut d = DenoService::new(inspect_brk);
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
    runtime.sync_ops_cache();

    // FIXME: Include these files in the snapshop
    let chisel = compile_ts_code_as_bytes(include_bytes!("chisel.js"))?;
    let api = compile_ts_code_as_bytes(include_bytes!("api.ts"))?;
    let chisel_path = "/chisel.js".to_string();

    {
        let mut code_map = d.module_loader.code_map.borrow_mut();
        code_map.insert(chisel_path.clone(), chisel);
        code_map.insert("/api.ts".to_string(), api);
    }

    worker
        .execute_main_module(
            &ModuleSpecifier::parse(&(DUMMY_PREFIX.to_string() + &chisel_path)).unwrap(),
        )
        .await?;
    Ok(d)
}

pub(crate) async fn init_deno(inspect_brk: bool) -> Result<()> {
    let service = Rc::new(RefCell::new(create_deno(inspect_brk).await?));
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

thread_local! {
    static CURRENT_REQUEST_PATH : RefCell<RequestPath> = RefCell::new(Default::default());
}

pub(crate) fn current_api_version() -> String {
    CURRENT_REQUEST_PATH.with(|p| {
        let x = p.borrow();
        x.api_version().to_string()
    })
}

fn set_current_path(current_path: String) {
    let rp = RequestPath::try_from(current_path.as_ref()).unwrap();

    CURRENT_REQUEST_PATH.with(|path| {
        let mut borrow = path.borrow_mut();
        *borrow = rp;
    });
}

struct RequestFuture<F> {
    request_path: String,
    inner: F,
}

impl<F: Future> Future for RequestFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, c: &mut Context<'_>) -> Poll<F::Output> {
        set_current_path(self.request_path.clone());
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
    // Set the current path to cover JS code that runs before
    // blocking. This in particular covers code that doesn't block at
    // all.
    set_current_path(path.clone());
    let result = get_result_aux(runtime, request_handler, req)?;
    let result = runtime.resolve_value(result);
    // We got here without blocking and now have a future representing
    // pending work for the endpoint. We might not get to that future
    // before the current path is changed, so wrap the future in a
    // RequestFuture that will reset the current path before polling.
    RequestFuture {
        request_path: path,
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

    let request_handler = service.handlers.get(&path).unwrap().func.clone().unwrap();
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

async fn get_endpoint(
    module_loader: &ModuleLoader,
    runtime: &mut JsRuntime,
    path: String,
    code: &VersionedCode,
) -> Result<v8::Global<v8::Function>> {
    // Modules are never unloaded, so we need to create an unique
    // path. This will not be a problem once we publish the entire app
    // at once, since then we can create a new isolate for it.
    let url = format!("{}/{}?ver={}", DUMMY_PREFIX, path, code.version);
    let url = Url::parse(&url).unwrap();

    module_loader
        .code_map
        .borrow_mut()
        .insert(path.clone(), code.code.clone());
    let promise = runtime.execute_script(&path, &format!("import(\"{}\")", url))?;
    let module = runtime.resolve_value(promise).await?;
    module_loader.code_map.borrow_mut().remove(&path);

    let scope = &mut runtime.handle_scope();
    let module = module
        .open(scope)
        .to_object(scope)
        .ok_or(Error::NotAResponse)?;
    let request_handler: v8::Local<v8::Function> = get_member(module, scope, "default")?;
    Ok(v8::Global::new(scope, request_handler))
}

pub(crate) async fn define_endpoint(path: String, code: String) -> Result<()> {
    let mut service = get();
    let service: &mut DenoService = &mut service;
    let mut entry = service.handlers.entry(path.clone());
    let version = match &entry {
        Entry::Vacant(_) => 0,
        Entry::Occupied(o) => o.get().version + 1,
    };
    let dummy = VersionedHandler {
        func: None,
        version,
    };
    let entry = match entry {
        Entry::Vacant(v) => v.insert(dummy),
        Entry::Occupied(ref mut o) => {
            let o = o.get_mut();
            *o = dummy;
            o
        }
    };
    let code = VersionedCode { code, version };
    let e = get_endpoint(
        &service.module_loader,
        &mut service.worker.js_runtime,
        path,
        &code,
    )
    .await?;
    entry.func = Some(e);
    Ok(())
}

pub(crate) fn define_type(ty: &ObjectType) -> Result<()> {
    let mut service = get();
    let runtime = &mut service.worker.js_runtime;
    let global_context = runtime.global_context();
    let scope = &mut runtime.handle_scope();
    let global_proxy = global_context.open(scope).global(scope);
    let chisel: v8::Local<v8::Object> = get_member(global_proxy, scope, "Chisel").unwrap();
    let api: v8::Local<v8::Object> = get_member(chisel, scope, "api")?;
    let table_func: v8::Local<v8::Function> = get_member(api, scope, "table")?;

    let mut fields = vec![];
    for f in &ty.fields {
        let name = v8::String::new(scope, &f.name).unwrap().into();
        let ty_name = f.type_.name();
        let ty_name = v8::String::new(scope, ty_name).unwrap().into();
        let tuple = v8::Array::new_with_elements(scope, &[name, ty_name]).into();
        fields.push(tuple);
    }

    let columns = v8::Array::new_with_elements(scope, &fields).into();
    let name = v8::String::new(scope, ty.name()).unwrap();
    let table = try_into_or(table_func.call(scope, api.into(), &[name.into(), columns]))?;

    global_proxy.set(scope, name.into(), table).unwrap();
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
