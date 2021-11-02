// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::Body;
use crate::runtime;
use crate::types::{ObjectType, Policies, Type, TypeSystemError};
use anyhow::{anyhow, Result};
use deno_broadcast_channel::InMemoryBroadcastChannel;
use deno_core::error::AnyError;
use deno_core::op_async;
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
use deno_runtime::inspector_server::InspectorServer;
use deno_runtime::permissions::Permissions;
use deno_runtime::worker::{MainWorker, WorkerOptions};
use deno_runtime::BootstrapOptions;
use deno_web::BlobStore;
use futures::stream;
use futures::stream::{try_unfold, Stream};
use futures::FutureExt;
use hyper::body::HttpBody;
use hyper::header::HeaderValue;
use hyper::Method;
use hyper::{Request, Response, StatusCode};
use once_cell::sync::Lazy;
use once_cell::unsync::OnceCell;
use rusty_v8 as v8;
use serde_json;
use sqlx::any::AnyRow;
use sqlx::Row;
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryInto;
use std::net::SocketAddr;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use url::Url;

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
    next_end_point_id: i32,
}

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error["Endpoint didn't produce a response"]]
    NotAResponse,
    #[error["Type name error; the .name key must have a string value"]]
    TypeName,
    #[error["Query execution error `{0}`"]]
    Query(#[from] crate::query::QueryError),
}

struct ModuleLoader {
    code_map: RefCell<HashMap<ModuleSpecifier, String>>,
}

static DUMMY_PATH: Lazy<Url> =
    Lazy::new(|| ModuleSpecifier::parse("file://$chisel$/main.js").unwrap());

impl deno_core::ModuleLoader for ModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        _referrer: &str,
        _is_main: bool,
    ) -> Result<ModuleSpecifier, AnyError> {
        Ok(ModuleSpecifier::parse(specifier)?)
    }

    fn load(
        &self,
        specifier: &ModuleSpecifier,
        _maybe_referrer: Option<ModuleSpecifier>,
        _is_dyn_import: bool,
    ) -> Pin<Box<ModuleSourceFuture>> {
        let code = self.code_map.borrow().get(specifier).unwrap().clone();
        let specifier = specifier.to_string();
        let f = || async {
            Ok(ModuleSource {
                code,
                module_url_specified: specifier.clone(),
                module_url_found: specifier,
            })
        };
        f().boxed_local()
    }
}

impl DenoService {
    pub fn new(inspect_brk: bool) -> Self {
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
            next_end_point_id: 0,
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
    let type_name = content["name"].as_str().ok_or(Error::TypeName)?;
    let runtime = &mut runtime::get().await;
    let ty = match runtime.type_system.lookup_type(type_name)? {
        Type::String => {
            return Err(TypeSystemError::ObjectTypeRequired(type_name.to_string()).into())
        }
        Type::Object(t) => t,
    };
    runtime
        .query_engine
        .add_row(&ty, &content["value"])
        .await
        .map_err(|e| e.into())
}

struct QueryStreamResource {
    #[allow(clippy::type_complexity)]
    stream: RefCell<Pin<Box<dyn stream::Stream<Item = Result<AnyRow, sqlx::Error>>>>>,
    policies: Policies,
    ty: ObjectType,
}

impl<'a> Resource for QueryStreamResource {}

async fn op_chisel_query_create(
    op_state: Rc<RefCell<OpState>>,
    type_name: String,
    _: (),
) -> Result<ResourceId, AnyError> {
    let mut policies = Policies::default();
    let runtime = &mut runtime::get().await;
    let ts = &runtime.type_system;
    let (stream, ty) = match ts.lookup_type(&type_name) {
        Ok(Type::Object(ty)) => {
            ts.get_policies(&ty, &mut policies);
            let query_engine = &mut runtime.query_engine;
            (query_engine.find_all(&ty), ty)
        }
        Ok(_) => {
            return Err(TypeSystemError::ObjectTypeRequired(type_name.to_string()).into());
        }
        Err(e) => {
            return Err(anyhow!("Failed to look up type {}: {}", type_name, e));
        }
    };
    let resource = QueryStreamResource {
        stream: RefCell::new(Box::pin(stream)),
        policies,
        ty,
    };
    let rid = op_state.borrow_mut().resource_table.add(resource);
    Ok(rid)
}

async fn op_chisel_query_next(
    state: Rc<RefCell<OpState>>,
    query_stream_rid: ResourceId,
    _: (),
) -> Result<Option<serde_json::Value>> {
    let resource: Rc<QueryStreamResource> = state.borrow().resource_table.get(query_stream_rid)?;
    let mut stream = resource.stream.borrow_mut();
    use futures::stream::StreamExt;

    if let Some(row) = stream.next().await {
        let row = row.unwrap();
        let mut v = serde_json::json!({});
        for field in &resource.ty.fields {
            let field_v: &str = row.try_get(&*field.name)?;
            v[&field.name] = serde_json::json!(field_v);
        }
        for (field, xform) in &resource.policies {
            v[field] = xform(v[field].take());
        }
        Ok(Some(v))
    } else {
        Ok(None)
    }
}

async fn create_deno(inspect_brk: bool) -> Result<DenoService> {
    let mut d = DenoService::new(inspect_brk);
    let worker = &mut d.worker;
    let runtime = &mut worker.js_runtime;

    // FIXME: Turn this into a deno extension
    runtime.register_op("chisel_read_body", op_async(op_chisel_read_body));
    runtime.register_op("chisel_store", op_async(op_chisel_store));
    runtime.register_op("chisel_query_create", op_async(op_chisel_query_create));
    runtime.register_op("chisel_query_next", op_async(op_chisel_query_next));
    runtime.sync_ops_cache();

    // FIXME: Include this js in the snapshop
    let code = std::str::from_utf8(include_bytes!("chisel.js"))?.to_string();
    d.module_loader
        .code_map
        .borrow_mut()
        .insert(DUMMY_PATH.clone(), code);

    worker.execute_main_module(&DUMMY_PATH).await?;
    Ok(d)
}

pub async fn init_deno(inspect_brk: bool) -> Result<()> {
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
    service: Rc<RefCell<DenoService>>,
) -> Result<Option<(Box<[u8]>, ())>> {
    let mut borrow = service.borrow_mut();
    let runtime = &mut borrow.worker.js_runtime;
    let js_promise = {
        let scope = &mut runtime.handle_scope();
        let reader = v8::Local::new(scope, reader.clone());
        let res = read
            .get(scope)
            .call(scope, reader, &[])
            .ok_or(Error::NotAResponse)?;
        v8::Global::new(scope, res)
    };
    let read_result = runtime.resolve_value(js_promise).await?;
    let scope = &mut runtime.handle_scope();
    let read_result = read_result
        .get(scope)
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
    service: Rc<RefCell<DenoService>>,
) -> Result<impl Stream<Item = Result<Box<[u8]>>>> {
    let scope = &mut runtime.handle_scope();
    let response = global_response
        .get(scope)
        .to_object(scope)
        .ok_or(Error::NotAResponse)?;

    let body: v8::Local<v8::Object> = get_member(response, scope, "body")?;
    let get_reader: v8::Local<v8::Function> = get_member(body, scope, "getReader")?;
    let reader: v8::Local<v8::Object> = try_into_or(get_reader.call(scope, body.into(), &[]))?;
    let read: v8::Local<v8::Function> = get_member(reader, scope, "read")?;
    let reader: v8::Local<v8::Value> = reader.into();
    let reader: v8::Global<v8::Value> = v8::Global::new(scope, reader);
    let read = v8::Global::new(scope, read);

    let stream = try_unfold((), move |_| {
        get_read_future(reader.clone(), read.clone(), service.clone())
    });
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

fn get_result(
    runtime: &mut JsRuntime,
    request_handler: v8::Global<v8::Function>,
    req: &mut Request<hyper::Body>,
) -> Result<v8::Global<v8::Value>> {
    let op_state = runtime.op_state();
    let global_context = runtime.global_context();
    let scope = &mut runtime.handle_scope();
    let global_proxy = global_context.get(scope).global(scope);

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

        let chisel: v8::Local<v8::Object> = get_member(global_proxy, scope, "Chisel").unwrap();
        let build: v8::Local<v8::Function> =
            get_member(chisel, scope, "buildReadableStreamForBody").unwrap();
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
        .get(scope)
        .call(scope, global_proxy.into(), &[request.into()])
        .ok_or(Error::NotAResponse)?;
    Ok(v8::Global::new(scope, result))
}

async fn run_js_aux(
    d: Rc<RefCell<DenoService>>,
    path: String,
    code: String,
    mut req: Request<hyper::Body>,
) -> Result<Response<Body>> {
    let service = &mut *d.borrow_mut();
    let worker = &mut service.worker;

    if service.inspector.is_some() {
        let runtime = &mut worker.js_runtime;
        runtime
            .inspector()
            .wait_for_session_and_break_on_next_statement();
    }

    // Modules are never unloaded, so we need to create an unique
    // path. This will not be a problem once we publish the entire app
    // at once, since then we can create a new isolate for it.
    let url = format!("file://$chisel{}$/path{}", service.next_end_point_id, path);
    let url = Url::parse(&url).unwrap();
    service
        .module_loader
        .code_map
        .borrow_mut()
        .insert(url.clone(), code);
    service.next_end_point_id += 1;

    let runtime = &mut service.worker.js_runtime;
    let ret = runtime.execute_script(&path, &format!("import(\"{}\")", url))?;
    let ret = runtime.resolve_value(ret).await?;
    let request_handler = {
        let scope = &mut runtime.handle_scope();
        let local = ret.get(scope).to_object(scope).unwrap();
        let request_handler: v8::Local<v8::Function> = get_member(local, scope, "default")?;
        v8::Global::<v8::Function>::new(scope, request_handler)
    };
    let result = get_result(runtime, request_handler, &mut req)?;

    let result = runtime.resolve_value(result).await?;
    let stream = get_read_stream(runtime, result.clone(), d.clone())?;
    let scope = &mut runtime.handle_scope();
    let response = result
        .get(scope)
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

    let body = builder.body(Body::Stream(Box::pin(stream)))?;
    Ok(body)
}

pub async fn run_js(
    path: String,
    code: String,
    req: Request<hyper::Body>,
) -> Result<Response<Body>> {
    DENO.with(|d| {
        let d = d.get().expect("Deno is not not yet inialized");
        run_js_aux(d.clone(), path, code, req)
    })
    .await
}
