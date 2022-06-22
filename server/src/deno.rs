// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::ApiService;
use crate::api::{response_template, Body, RequestPath};
use crate::auth::get_username_from_id;
use crate::datastore::crud;
use crate::datastore::engine::extract_transaction;
use crate::datastore::engine::IdTree;
use crate::datastore::engine::TransactionStatic;
use crate::datastore::engine::{QueryResults, ResultRow};
use crate::datastore::expr::Expr;
use crate::datastore::query::{Mutation, QueryOpChain, QueryPlan, RequestContext};
use crate::datastore::MetaService;
use crate::datastore::QueryEngine;
use crate::policies::Policies;
use crate::rcmut::RcMut;
use crate::types::Type;
use crate::types::TypeSystem;
use crate::types::TypeSystemError;
use crate::vecmap::VecMap;
use crate::JsonObject;
use anyhow::{anyhow, Context as AnyhowContext, Result};
use api::chisel_js;
use api::endpoint_js;
use api::worker_js;
use async_channel::Receiver;
use async_channel::Sender;
use deno_core::error::AnyError;
use deno_core::futures;
use deno_core::op;
use deno_core::serde_v8;
use deno_core::url::Url;
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
use futures::stream::{try_unfold, Stream};
use futures::task::LocalFutureObj;
use futures::{future, FutureExt};
use hyper::body::HttpBody;
use hyper::Method;
use hyper::Uri;
use hyper::{Request, Response, StatusCode};
use log::debug;
use once_cell::sync::Lazy;
use once_cell::unsync::OnceCell;
use pin_project::pin_project;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::convert::TryInto;
use std::fmt::Debug;
use std::future::Future;
use std::io::Read;
use std::net::SocketAddr;
use std::ops::DerefMut;
use std::pin::Pin;
use std::rc::Rc;
use std::rc::Weak;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::{Context, Poll};
use utils::without_extension;

enum WorkerMsg {
    SetMeta(MetaService),
    HandleRequest(Request<hyper::Body>),
    SetTypeSystem(TypeSystem),
    RemoveTypeVersion(String),
    SetQueryEngine(Arc<QueryEngine>),
    SetPolicies(Policies),
    MutatePolicies(Box<dyn FnOnce(&mut Policies) + Send>),
    SetCurrentSecrets(JsonObject),
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

    import_endpoints: v8::Global<v8::Function>,
    activate_endpoint: v8::Global<v8::Function>,
    call_handler: v8::Global<v8::Function>,
    read_worker_channel: v8::Global<v8::Function>,
    end_of_request: v8::Global<v8::Function>,

    to_worker: Sender<WorkerMsg>,
    worker_channel_id: u32,
}

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error["Endpoint didn't produce a response"]]
    NotAResponse,
}

struct ModuleLoaderInner {
    code_map: HashMap<Url, String>,
}

impl ModuleLoaderInner {
    fn insert_path(&mut self, path: &str, code: &str) {
        let url = Url::parse(&format!("file://{}", path)).unwrap();
        self.code_map.insert(url, code.to_string());
    }
}

struct ModuleLoader {
    inner: Arc<std::sync::Mutex<ModuleLoaderInner>>,
}

fn wrap(specifier: &ModuleSpecifier, code: String) -> Result<ModuleSource> {
    Ok(ModuleSource {
        code: code.into_bytes().into_boxed_slice(),
        module_type: ModuleType::JavaScript,
        module_url_specified: specifier.to_string(),
        module_url_found: specifier.to_string(),
    })
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
        maybe_referrer: Option<ModuleSpecifier>,
        _is_dyn_import: bool,
    ) -> Pin<Box<ModuleSourceFuture>> {
        let handle = self.inner.lock().unwrap();
        if let Some(code) = handle.code_map.get(specifier).cloned() {
            let specifier = specifier.clone();
            async move { wrap(&specifier, code) }.boxed_local()
        } else {
            let err = anyhow!(
                "chiseld cannot load module {} at runtime{}",
                specifier,
                maybe_referrer
                    .map(|url| format!(" (referrer: {})", url))
                    .unwrap_or_default(),
            );
            async move { Err(err) }.boxed_local()
        }
    }
}

fn build_extensions() -> Vec<Extension> {
    vec![Extension::builder()
        .ops(vec![
            op_format_file_name::decl(),
            op_chisel_read_body::decl(),
            op_chisel_store::decl(),
            op_chisel_insert_junction::decl(),
            op_chisel_entity_delete::decl(),
            op_chisel_crud_delete::decl(),
            op_chisel_get_secret::decl(),
            op_chisel_crud_query::decl(),
            op_chisel_relational_query_create::decl(),
            op_chisel_query_next::decl(),
            op_chisel_commit_transaction::decl(),
            op_chisel_rollback_transaction::decl(),
            op_chisel_create_transaction::decl(),
            op_chisel_init_worker::decl(),
            op_chisel_read_worker_channel::decl(),
            op_chisel_start_request::decl(),
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
            format_js_error_fn: None,
            source_map_getter: None,
            stdio: Default::default(),
            bootstrap: bootstrap.clone(),
            extensions,
            unsafely_ignore_certificate_errors: None,
            root_cert_store: None,
            seed: None,
            module_loader,
            create_web_worker_cb,
            preload_module_cb: preload_module_cb.clone(),
            worker_type: args.worker_type,
            maybe_inspector_server: maybe_inspector_server.clone(),
            get_error_class_fn: None,
            blob_store: Default::default(),
            broadcast_channel: Default::default(),
            shared_array_buffer_store: None,
            compiled_wasm_module_store: None,
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

type Channel = Receiver<WorkerMsg>;
type GlobalChannels = VecMap<Channel>;

static GLOBAL_WORKER_CHANNELS: Lazy<Mutex<GlobalChannels>> =
    Lazy::new(|| Mutex::new(GlobalChannels::new()));

thread_local! {
     static WORKER_CHANNEL: OnceCell<Channel> = OnceCell::new();
}

impl DenoService {
    pub(crate) async fn new(inspect_brk: bool) -> (Self, v8::Global<v8::Function>) {
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
            user_agent: "hello_runtime".to_string(),
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
            format_js_error_fn: None,
            source_map_getter: None,
            stdio: Default::default(),
            bootstrap,
            extensions,
            unsafely_ignore_certificate_errors: None,
            root_cert_store: None,
            seed: None,
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
            handle.insert_path(main_path, include_str!("./main.js"));
            handle.insert_path("/chisel.ts", chisel_js());
            handle.insert_path(endpoint_path, endpoint_js());
            handle.insert_path("/worker.js", worker_js());
        }

        let main_url = Url::parse(&format!("file://{}", main_path)).unwrap();
        worker.execute_main_module(&main_url).await.unwrap();

        let (
            import_endpoints,
            activate_endpoint,
            call_handler,
            init_worker,
            read_worker_channel,
            end_of_request,
        ) = {
            let runtime = &mut worker.js_runtime;
            let promise = runtime
                .execute_script(main_path, &format!("import(\"file://{}\")", endpoint_path))
                .unwrap();
            let module = runtime.resolve_value(promise).await.unwrap();
            let scope = &mut runtime.handle_scope();
            let module = v8::Local::new(scope, module).try_into().unwrap();
            let import_endpoints: v8::Local<v8::Function> =
                get_member(module, scope, "importEndpoints").unwrap();
            let import_endpoints = v8::Global::new(scope, import_endpoints);
            let activate_endpoint: v8::Local<v8::Function> =
                get_member(module, scope, "activateEndpoint").unwrap();
            let activate_endpoint = v8::Global::new(scope, activate_endpoint);
            let call_handler: v8::Local<v8::Function> =
                get_member(module, scope, "callHandler").unwrap();
            let call_handler = v8::Global::new(scope, call_handler);
            let init_worker: v8::Local<v8::Function> =
                get_member(module, scope, "initWorker").unwrap();
            let init_worker = v8::Global::new(scope, init_worker);
            let read_worker_channel: v8::Local<v8::Function> =
                get_member(module, scope, "readWorkerChannel").unwrap();
            let read_worker_channel = v8::Global::new(scope, read_worker_channel);
            let end_of_request: v8::Local<v8::Function> =
                get_member(module, scope, "endOfRequest").unwrap();
            let end_of_request = v8::Global::new(scope, end_of_request);

            (
                import_endpoints,
                activate_endpoint,
                call_handler,
                init_worker,
                read_worker_channel,
                end_of_request,
            )
        };

        let (to_worker_sender, to_worker_receiver) = async_channel::bounded(1);
        let mut map = GLOBAL_WORKER_CHANNELS.lock().unwrap();
        let worker_channel_id = map.push(to_worker_receiver) as u32;

        (
            Self {
                worker,
                inspector,
                module_loader: inner,
                import_endpoints,
                activate_endpoint,
                call_handler,
                to_worker: to_worker_sender,
                worker_channel_id,
                read_worker_channel,
                end_of_request,
            },
            init_worker,
        )
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

/// RequestContext corresponds to `requestContext` structure used in chisel.ts.
#[derive(Deserialize)]
struct ChiselRequestContext {
    /// Current URL path.
    path: String,
    /// Current HTTP method.
    #[serde(rename = "method")]
    _method: String,
    /// Current HTTP request headers.
    headers: HashMap<String, String>,
    /// Schema version to be used with the request.
    #[serde(rename = "apiVersion")]
    api_version: String,
    /// Current user ID.
    #[serde(rename = "userId")]
    user_id: Option<String>,
}

impl RequestContext<'_> {
    fn new<'a>(
        policies: &'a Policies,
        ts: &'a TypeSystem,
        context: ChiselRequestContext,
    ) -> RequestContext<'a> {
        RequestContext {
            policies,
            ts,
            api_version: context.api_version,
            user_id: context.user_id,
            path: context.path,
            headers: context.headers,
        }
    }
}

#[derive(Deserialize)]
struct StoreContent {
    name: String,
    value: JsonObject,
}

fn is_auth_path(api_version: &str, path: &str) -> bool {
    api_version == "__chiselstrike" && path.starts_with("/auth/")
}

#[op]
async fn op_chisel_store(
    state: Rc<RefCell<OpState>>,
    content: StoreContent,
    c: ChiselRequestContext,
) -> Result<IdTree> {
    let type_name = &content.name;
    let value = &content.value;

    let (query_engine, ty) = {
        let state = state.borrow();
        let ty = match current_type_system(&state).lookup_type(type_name, &c.api_version) {
            Ok(Type::Entity(ty)) => ty,
            _ => anyhow::bail!("Cannot save into type {}.", type_name),
        };
        if ty.is_auth() && !is_auth_path(&c.api_version, &c.path) {
            anyhow::bail!("Cannot save into type {}.", type_name);
        }

        let query_engine = query_engine_arc(&state);
        (query_engine, ty)
    };
    let transaction = {
        let state = state.borrow();
        current_transaction(&state)
    };
    let mut transaction = transaction.lock().await;

    {
        let state = state.borrow();
        let ts = current_type_system(&state);
        query_engine.add_row(&ty, value, Some(transaction.deref_mut()), ts)
    }
    .await
}

#[derive(Deserialize)]
struct JunctionElement {
    #[serde(rename = "parentEntity")]
    parent_entity: String,
    #[serde(rename = "parentField")]
    parent_field: String,
    #[serde(rename = "parentId")]
    parent_id: String,
    #[serde(rename = "elementId")]
    element_id: String,
}

#[op]
async fn op_chisel_insert_junction(
    state: Rc<RefCell<OpState>>,
    je: JunctionElement,
    c: ChiselRequestContext,
) -> Result<()> {
    let entity_name = &je.parent_entity;

    let (query_engine, entity) = {
        let state = state.borrow();
        let entity = current_type_system(&state).lookup_entity(entity_name, &c.api_version)?;

        let query_engine = query_engine_arc(&state);
        (query_engine, entity)
    };
    let transaction = {
        let state = state.borrow();
        current_transaction(&state)
    };
    let mut transaction = transaction.lock().await;
    query_engine
        .add_junction_element(
            &entity,
            &je.parent_field,
            &je.parent_id,
            &je.element_id,
            transaction.deref_mut(),
        )
        .await
    // .context("failed to insert junction element")
}

#[derive(Deserialize)]
struct DeleteParams {
    #[serde(rename = "typeName")]
    type_name: String,
    #[serde(rename = "filterExpr")]
    filter_expr: Option<Expr>,
}

#[op]
async fn op_chisel_entity_delete(
    state: Rc<RefCell<OpState>>,
    params: DeleteParams,
    context: ChiselRequestContext,
) -> Result<()> {
    let mutation = {
        let state = state.borrow_mut();
        Mutation::delete_from_expr(
            &RequestContext::new(
                current_policies(&state),
                current_type_system(&state),
                context,
            ),
            &params.type_name,
            &params.filter_expr,
        )
        .context(
            "failed to construct delete expression from JSON passed to `op_chisel_entity_delete`",
        )?
    };
    let query_engine = query_engine_arc(&state.borrow());
    let transaction = {
        let state = state.borrow();
        current_transaction(&state)
    };
    let mut transaction = transaction.lock().await;
    query_engine
        .mutate_with_transaction(mutation, &mut transaction)
        .await?;

    Ok(())
}

#[derive(Deserialize)]
struct CrudDeleteParams {
    #[serde(rename = "typeName")]
    type_name: String,
    url: String,
}

#[op]
async fn op_chisel_crud_delete(
    state: Rc<RefCell<OpState>>,
    params: CrudDeleteParams,
    context: ChiselRequestContext,
) -> Result<()> {
    let mutation = {
        let state = state.borrow_mut();
        crud::delete_from_url(
            &RequestContext::new(
                current_policies(&state),
                current_type_system(&state),
                context,
            ),
            &params.type_name,
            &params.url,
        )
        .context(
            "failed to construct delete expression from JSON passed to `op_chisel_crud_delete`",
        )?
    };
    let query_engine = {
        let state = state.borrow();
        query_engine_arc(&state).clone()
    };

    let transaction = query_engine.clone().start_transaction_static().await?;

    let mut guard = transaction.lock().await;
    query_engine
        .mutate_with_transaction(mutation, &mut guard)
        .await?;

    drop(guard);

    QueryEngine::commit_transaction_static(transaction).await?;

    Ok(())
}

type DbStream = RefCell<QueryResults>;

struct QueryStreamResource {
    stream: DbStream,
    cancel: CancelHandle,
}

impl Resource for QueryStreamResource {
    fn close(self: Rc<Self>) {
        self.cancel.cancel();
    }
}

#[op(v8)]
fn op_chisel_get_secret<'a>(
    scope: &mut v8::HandleScope<'a>,
    op_state: &mut OpState,
    key: String,
) -> serde_v8::Value<'a> {
    let secrets = current_secrets(op_state);
    match secrets.get(&key).cloned() {
        None => {
            let u = v8::undefined(scope);
            serde_v8::from_v8(scope, u.into()).unwrap()
        }
        Some(v) => {
            let v = serde_v8::to_v8(scope, v).unwrap();
            serde_v8::from_v8(scope, v).unwrap()
        }
    }
}

#[op]
async fn op_chisel_crud_query(
    state: Rc<RefCell<OpState>>,
    params: crud::QueryParams,
    context: ChiselRequestContext,
) -> Result<JsonObject> {
    // Contextualize stream creation to prevent state RC borrow living across await
    {
        let op_state = &state.borrow();
        let transaction = current_transaction(op_state);
        let query_engine = query_engine_arc(op_state);

        crud::run_query(
            &RequestContext::new(
                current_policies(op_state),
                current_type_system(op_state),
                context,
            ),
            params,
            query_engine,
            transaction,
        )
    }
    .await
}

#[op]
fn op_chisel_relational_query_create(
    op_state: &mut OpState,
    op_chain: QueryOpChain,
    context: ChiselRequestContext,
) -> Result<ResourceId> {
    let query_plan = QueryPlan::from_op_chain(
        &RequestContext::new(
            current_policies(op_state),
            current_type_system(op_state),
            context,
        ),
        op_chain,
    )?;
    create_query(op_state, query_plan)
}

fn create_query(op_state: &mut OpState, query_plan: QueryPlan) -> Result<ResourceId> {
    let transaction = current_transaction(op_state);
    let query_engine = query_engine_arc(op_state);
    let stream = query_engine.query(transaction, query_plan)?;
    let resource = QueryStreamResource {
        stream: RefCell::new(stream),
        cancel: Default::default(),
    };
    let rid = op_state.resource_table.add(resource);
    Ok(rid)
}

// A future that resolves when this stream next element is available.
struct QueryNextFuture {
    resource: Weak<QueryStreamResource>,
}

impl Future for QueryNextFuture {
    type Output = Option<Result<ResultRow>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.resource.upgrade() {
            Some(rc) => {
                let mut stream = rc.stream.borrow_mut();
                let stream: &mut QueryResults = &mut stream;
                Pin::new(stream).poll_next(cx)
            }
            None => Poll::Ready(Some(Err(anyhow!("Closed resource")))),
        }
    }
}

#[op]
async fn op_chisel_query_next(
    state: Rc<RefCell<OpState>>,
    query_stream_rid: ResourceId,
) -> Result<Option<ResultRow>> {
    let (resource, cancel) = {
        let rc: Rc<QueryStreamResource> = state.borrow().resource_table.get(query_stream_rid)?;
        let cancel = RcRef::map(&rc, |r| &r.cancel);
        (Rc::downgrade(&rc), cancel)
    };
    let fut = QueryNextFuture { resource };
    let fut = fut.or_cancel(cancel);
    if let Some(row) = fut.await? {
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

#[op]
fn op_chisel_init_worker(id: u32) {
    let mut map = GLOBAL_WORKER_CHANNELS.lock().unwrap();
    let channel = map.remove(id as usize).unwrap();
    WORKER_CHANNEL.with(|d| {
        d.set(channel).unwrap();
    });
}

#[op]
async fn op_chisel_read_worker_channel(state: Rc<RefCell<OpState>>) -> Result<()> {
    let receiver = WORKER_CHANNEL.with(|d| d.get().unwrap().clone());
    let msg = receiver.recv().await.unwrap();

    let mut state = state.borrow_mut();
    let state = &mut state;
    match msg {
        WorkerMsg::SetMeta(meta) => state.put::<Rc<MetaService>>(Rc::new(meta)),
        WorkerMsg::HandleRequest(_req) => unreachable!("Wrong message"),
        WorkerMsg::SetTypeSystem(type_system) => state.put(type_system),
        WorkerMsg::RemoveTypeVersion(version) => {
            state.borrow_mut::<TypeSystem>().versions.remove(&version);
        }
        WorkerMsg::SetQueryEngine(query_engine) => state.put(query_engine),
        WorkerMsg::SetPolicies(policies) => state.put(policies),
        WorkerMsg::MutatePolicies(func) => func(state.borrow_mut()),
        WorkerMsg::SetCurrentSecrets(secretes) => state.put(secretes),
    }

    Ok(())
}

thread_local! {
    static REJECTED: RefCell<HashSet<v8::Global<v8::Promise>>> = RefCell::new(HashSet::new());
}

pub(crate) fn shutdown() {
    REJECTED.with(|r| {
        assert!(r.borrow().is_empty());
    });
}

extern "C" fn promise_reject_callback(message: v8::PromiseRejectMessage) {
    use v8::PromiseRejectEvent::*;

    let scope = &mut unsafe { v8::CallbackScope::new(&message) };
    let promise = message.get_promise();
    let promise = v8::Global::new(scope, promise);
    match message.get_event() {
        PromiseRejectWithNoHandler => {
            REJECTED.with(|r| {
                r.borrow_mut().insert(promise);
            });
        }
        PromiseHandlerAddedAfterReject => {
            REJECTED.with(|r| {
                r.borrow_mut().remove(&promise);
            });
        }
        PromiseRejectAfterResolved => {}
        PromiseResolveAfterResolved => {}
    }
}

pub(crate) async fn init_deno(inspect_brk: bool) -> Result<()> {
    let (service, init_worker) = DenoService::new(inspect_brk).await;
    DENO.with(|d| {
        d.set(Rc::new(RefCell::new(service)))
            .map_err(|_| ())
            .expect("Deno is already initialized.");
    });

    let mut service = get();
    let service: &mut DenoService = &mut service;
    let runtime = &mut service.worker.js_runtime;
    let scope = &mut runtime.handle_scope();
    scope.set_promise_reject_callback(promise_reject_callback);
    let undefined = v8::undefined(scope).into();
    let id = v8::Number::new(scope, service.worker_channel_id as f64).into();
    init_worker
        .open(scope)
        .call(scope, undefined, &[id])
        .unwrap();
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

fn to_v8_func<'a>(
    scope: &mut v8::HandleScope<'a>,
    func: impl v8::MapFnTo<v8::FunctionCallback>,
) -> v8::Local<'a, v8::Function> {
    let func = v8::FunctionTemplate::new(scope, func);
    func.get_function(scope).unwrap()
}

// This has to be a macro because rust doesn't support &str const
// generic parameters.
macro_rules! wrap_object {
    ( $scope:expr, $key:expr) => {{
        let func = |scope: &mut v8::HandleScope,
                    args: v8::FunctionCallbackArguments,
                    mut rv: v8::ReturnValue| {
            let arg = args.get(0);
            let ret = v8::Object::new(scope);
            let key = v8::String::new(scope, $key).unwrap().into();
            ret.set(scope, key, arg);
            rv.set(ret.into());
        };
        to_v8_func($scope, func)
    }};
}

async fn resolve_promise(js_promise: v8::Global<v8::Value>) -> Result<v8::Global<v8::Value>> {
    // We have to make sure no exceptions are produced without a
    // handler. The way we do that is by wrapping the produced object
    // in then2 and then extracting the produced value or error, which
    // is mapped to a Result.
    let js_promise = {
        let mut service = get();
        let runtime = &mut service.worker.js_runtime;
        let scope = &mut runtime.handle_scope();
        let local = v8::Local::new(scope, js_promise.clone());
        let local: v8::Local<v8::Promise> = local.try_into().unwrap();
        let on_fulfilled = wrap_object!(scope, "value");
        let on_rejected = wrap_object!(scope, "error");
        let promise = local.then2(scope, on_fulfilled, on_rejected).unwrap();
        let promise: v8::Local<v8::Value> = promise.into();
        v8::Global::new(scope, promise)
    };

    let obj = ResolveFuture { js_promise }.await.unwrap();

    let mut service = get();
    let runtime = &mut service.worker.js_runtime;
    let scope = &mut runtime.handle_scope();
    let obj = obj.open(scope).to_object(scope).unwrap();
    let key = v8::String::new(scope, "value").unwrap().into();
    if obj.has(scope, key).unwrap() {
        let v = obj.get(scope, key).unwrap();
        return Ok(v8::Global::new(scope, v));
    }
    let key = v8::String::new(scope, "error").unwrap().into();
    assert!(obj.has(scope, key).unwrap());
    anyhow::bail!(obj.get(scope, key).unwrap().to_rust_string_lossy(scope));
}

type ReadFutureState = v8::Global<v8::Function>;
async fn get_read_future(read: ReadFutureState) -> Result<Option<(Box<[u8]>, ReadFutureState)>> {
    let js_promise = {
        let mut service = get();
        let runtime = &mut service.worker.js_runtime;
        let scope = &mut runtime.handle_scope();
        let undefined = v8::undefined(scope).into();
        let res = read
            .open(scope)
            .call(scope, undefined, &[])
            .ok_or(Error::NotAResponse)?;
        v8::Global::new(scope, res)
    };
    let read_result = resolve_promise(js_promise).await?;
    let mut service = get();
    let runtime = &mut service.worker.js_runtime;
    let scope = &mut runtime.handle_scope();
    let read_result = v8::Local::new(scope, read_result);
    if read_result.is_undefined() {
        return Ok(None);
    }
    let value: v8::Local<v8::ArrayBufferView> = read_result.try_into()?;
    let size = value.byte_length();
    // FIXME: We might want to use an uninitialized buffer.
    let mut buffer = vec![0; size];
    let copied = value.copy_contents(&mut buffer);
    // FIXME: Check in V8 to see when this might fail
    assert!(copied == size);
    Ok(Some((buffer.into_boxed_slice(), read)))
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

    let read = get_member::<v8::Local<v8::Function>>(response, scope, "read")?;
    let read = v8::Global::new(scope, read);
    Ok(try_unfold(read, get_read_future))
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

fn current_policies(st: &OpState) -> &Policies {
    st.borrow()
}

fn is_allowed_by_policy(
    state: &OpState,
    api_version: &str,
    username: Option<String>,
    path: &std::path::Path,
) -> Result<bool> {
    let policies = current_policies(state);
    match policies.versions.get(api_version) {
        None => Err(anyhow!(
            "found a route, but no version object for {}/{}",
            api_version,
            path.display()
        )),
        Some(x) => Ok(x.user_authorization.is_allowed(username, path)),
    }
}

async fn mutate_policies_impl(func: Box<dyn FnOnce(&mut Policies) + Send>) {
    to_worker(WorkerMsg::MutatePolicies(func)).await;
}

pub(crate) async fn mutate_policies<F>(func: F)
where
    F: FnOnce(&mut Policies) + Send + 'static,
{
    mutate_policies_impl(Box::new(func)).await;
}

pub(crate) async fn set_policies(policies: Policies) {
    to_worker(WorkerMsg::SetPolicies(policies)).await;
}

fn take_current_transaction(state: &mut OpState) -> TransactionStatic {
    state.take()
}

fn current_transaction(st: &OpState) -> TransactionStatic {
    st.borrow::<TransactionStatic>().clone()
}

fn set_current_transaction(st: &mut OpState, transaction: TransactionStatic) {
    assert!(!st.has::<TransactionStatic>());
    st.put(transaction);
}

fn current_secrets(st: &OpState) -> &JsonObject {
    st.borrow()
}

fn current_type_system(st: &OpState) -> &TypeSystem {
    st.borrow()
}

pub(crate) async fn set_meta(meta: MetaService) {
    to_worker(WorkerMsg::SetMeta(meta)).await;
}

pub(crate) fn query_engine_arc(st: &OpState) -> Arc<QueryEngine> {
    st.borrow::<Arc<QueryEngine>>().clone()
}

pub(crate) async fn set_query_engine(query_engine: Arc<QueryEngine>) {
    to_worker(WorkerMsg::SetQueryEngine(query_engine)).await;
}

pub(crate) fn lookup_builtin_type(
    state: &OpState,
    type_name: &str,
) -> Result<Type, TypeSystemError> {
    let type_system = current_type_system(state);
    type_system.lookup_builtin_type(type_name)
}

pub(crate) async fn remove_type_version(version: &str) {
    to_worker(WorkerMsg::RemoveTypeVersion(version.to_string())).await;
}

async fn to_worker(msg: WorkerMsg) {
    let promise = {
        let sender = get().to_worker.clone();
        sender.send(msg).await.unwrap();
        let mut service = get();
        let service: &mut DenoService = &mut service;
        let runtime = &mut service.worker.js_runtime;
        let scope = &mut runtime.handle_scope();
        let undefined = v8::undefined(scope).into();
        let promise = service
            .read_worker_channel
            .open(scope)
            .call(scope, undefined, &[])
            .unwrap();
        v8::Global::new(scope, promise)
    };
    resolve_promise(promise).await.unwrap();
}

pub(crate) async fn set_type_system(type_system: TypeSystem) {
    to_worker(WorkerMsg::SetTypeSystem(type_system)).await;
}

pub(crate) async fn update_secrets(secrets: JsonObject) {
    to_worker(WorkerMsg::SetCurrentSecrets(secrets.clone())).await;
}

#[op]
async fn op_chisel_commit_transaction(state: Rc<RefCell<OpState>>) -> Result<()> {
    let transaction = {
        let mut state = state.borrow_mut();
        take_current_transaction(&mut state)
    };
    crate::datastore::QueryEngine::commit_transaction_static(transaction).await?;
    Ok(())
}

#[op]
fn op_chisel_rollback_transaction(state: &mut OpState) -> Result<()> {
    let transaction = take_current_transaction(state);
    // Check that this is the last reference to the transaction.
    let transaction = extract_transaction(transaction);
    // Drop the transaction, causing it to rollback.
    drop(transaction);
    Ok(())
}

#[op]
async fn op_chisel_create_transaction(state: Rc<RefCell<OpState>>) -> Result<()> {
    let qe = query_engine_arc(&state.borrow());
    let transaction = qe.start_transaction_static().await?;
    set_current_transaction(&mut state.borrow_mut(), transaction);
    Ok(())
}

#[derive(Serialize)]
struct ResponseParts {
    status: u16,
    body: ZeroCopyBuf,
    headers: Vec<(String, String)>,
}

struct RequestHandler {
    id: u32,
}

impl Drop for RequestHandler {
    fn drop(&mut self) {
        let mut service = get();
        let service: &mut DenoService = &mut service;
        let runtime = &mut service.worker.js_runtime;
        let scope = &mut runtime.handle_scope();
        let end_of_request = service.end_of_request.open(scope);
        let undefined = v8::undefined(scope).into();
        let id = v8::Number::new(scope, self.id as f64).into();
        end_of_request.call(scope, undefined, &[id]).unwrap();
    }
}

#[pin_project]
struct EndReqStream<S> {
    #[pin]
    inner: S,
    req: RequestHandler,
}

impl<S> Stream for EndReqStream<S>
where
    S: Stream,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.project().inner.poll_next(cx)
    }
}

// FIXME: It would probably be cleaner to move more of this to
// javascript.
async fn special_response(
    state: Rc<RefCell<OpState>>,
    req: &Request<hyper::Body>,
    userid: &Option<String>,
) -> Result<Option<Response<Body>>> {
    let req_path = req.uri().path();
    // TODO: Make this optional, for users who want to reject some OPTIONS requests.
    if req.method() == "OPTIONS" {
        // Makes CORS preflights pass.
        return Ok(Some(Response::builder().body("ok".to_string().into())?));
    }
    if req_path.starts_with("/__chiselstrike/auth/") {
        let auth_header = req.headers().get("ChiselAuth");
        if auth_header.is_none() {
            return Ok(Some(ApiService::forbidden("AuthSecret")?));
        }
        let expected_secret = current_secrets(&state.borrow())
            .get("CHISELD_AUTH_SECRET")
            .cloned();
        match (expected_secret, auth_header) {
            (Some(serde_json::Value::String(s)), Some(h)) if s != *h => {
                return Ok(Some(ApiService::forbidden("Fundamental auth")?))
            }
            _ => (),
        }
    } else {
        let username = get_username_from_id(state.clone(), userid.clone()).await;
        let rp = match RequestPath::try_from(req_path) {
            Ok(rp) => rp,
            Err(_) => return Ok(Some(ApiService::not_found()?)),
        };
        let is_allowed = is_allowed_by_policy(
            &state.borrow(),
            rp.api_version(),
            username,
            rp.path().as_ref(),
        )?;
        if !is_allowed {
            return Ok(Some(ApiService::forbidden("Unauthorized user\n")?));
        }
    }
    Ok(None)
}

async fn convert_response(mut res: Response<Body>) -> Result<ResponseParts> {
    let status = res.status().as_u16();
    let res_body = res.body_mut();

    let mut body = vec![];
    while let Some(data) = res_body.data().await {
        let mut data = data?;
        data.read_to_end(&mut body)?;
    }

    let res_headers = res.headers();
    let mut headers = Vec::new();
    for (k, v) in res_headers {
        headers.push((k.to_string(), v.to_str()?.to_string()));
    }
    Ok(ResponseParts {
        status,
        body: body.into(),
        headers,
    })
}

pub(crate) async fn run_js(path: String, req: Request<hyper::Body>) -> Result<Response<Body>> {
    thread_local! {
        static NEXT_REQUEST_ID: Cell<u32> = Cell::new(0);
    }

    let id = NEXT_REQUEST_ID.with(|x| {
        let v = x.get();
        x.set(v + 1);
        v
    });
    let request_handler = RequestHandler { id };

    {
        let mut service = get();
        if service.inspector.is_some() {
            let runtime = &mut service.worker.js_runtime;
            runtime
                .inspector()
                .wait_for_session_and_break_on_next_statement();
        }
    }

    let sender = get().to_worker.clone();
    sender.send(WorkerMsg::HandleRequest(req)).await.unwrap();

    let result = {
        let mut service = get();
        let service: &mut DenoService = &mut service;
        let runtime = &mut service.worker.js_runtime;
        let scope = &mut runtime.handle_scope();

        let path = RequestPath::try_from(path.as_ref()).unwrap();
        let call_handler = service.call_handler.open(scope);
        let undefined = v8::undefined(scope).into();
        let api_version = v8::String::new(scope, path.api_version()).unwrap().into();
        let path = v8::String::new(scope, path.path()).unwrap().into();
        let id = v8::Number::new(scope, id as f64).into();
        let result = call_handler
            .call(scope, undefined, &[path, api_version, id])
            .unwrap();
        v8::Global::new(scope, result)
    };
    let result = resolve_promise(result).await?;

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
        let stream = EndReqStream {
            inner: stream,
            req: request_handler,
        };

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

    Ok(body)
}

#[derive(Serialize)]
struct StartRequest {
    body_rid: Option<u32>,
    headers: HashMap<String, String>,
    method: String,
    url: String,
    userid: Option<String>,
}

async fn handle_request(
    state: Rc<RefCell<OpState>>,
    userid: Option<String>,
    req: Request<hyper::Body>,
) -> Result<StartRequest> {
    // FIXME: this request conversion is probably simplistic. Check deno/ext/http/lib.rs

    // Hyper gives us a URL with just the path, make it a full URL
    // before passing it to deno.
    // FIXME: Use the real values for this server.
    let url = Uri::builder()
        .scheme("http")
        .authority("chiselstrike.com")
        .path_and_query(req.uri().path_and_query().unwrap().clone())
        .build()
        .unwrap();
    let url = url.to_string();
    let method = req.method();

    let mut headers: HashMap<String, String> = HashMap::new();
    for (k, v) in req.headers().iter() {
        let k = k.as_str();
        let v = v.to_str()?;
        headers.insert(k.to_string(), v.to_string());
    }

    let has_body = method != Method::GET && method != Method::HEAD;
    let method = method.as_str().to_string();
    let body_rid = if has_body {
        let body = req.into_body();
        let resource = BodyResource {
            body: RefCell::new(body),
            cancel: Default::default(),
        };
        let rid = state.borrow_mut().resource_table.add(resource);
        Some(rid)
    } else {
        None
    };

    Ok(StartRequest {
        body_rid,
        headers,
        method,
        url,
        userid,
    })
}

#[derive(Serialize)]
enum StartRequestRes {
    Js(StartRequest),
    Special(ResponseParts),
}

#[op]
async fn op_chisel_start_request(state: Rc<RefCell<OpState>>) -> Result<StartRequestRes> {
    let receiver = WORKER_CHANNEL.with(|d| d.get().unwrap().clone());
    let req = match receiver.recv().await {
        Ok(WorkerMsg::HandleRequest(req)) => req,
        _ => unreachable!("Wrong message"),
    };
    let userid = match req.headers().get("ChiselUID").map(|v| v.to_str()) {
        Some(Ok(str)) => Some(str.to_string()),
        Some(Err(e)) => {
            warn!(
                "Weird bytes in ChiselUID value on request {:?}, error {:?}",
                req, e
            );
            None
        }
        None => None,
    };
    if let Some(resp) = special_response(state.clone(), &req, &userid).await? {
        let resp = convert_response(resp).await?;
        return Ok(StartRequestRes::Special(resp));
    }

    Ok(StartRequestRes::Js(
        handle_request(state, userid, req).await?,
    ))
}

fn get() -> RcMut<DenoService> {
    DENO.with(|x| {
        let rc = x.get().expect("Runtime is not yet initialized.").clone();
        RcMut::new(rc)
    })
}

pub(crate) fn endpoint_path_from_source_path(path: &str) -> String {
    // The source path format is /api_version/endpoints/rest.js. The endpoint is /api_version/rest.
    let mut iter = path.splitn(4, '/');
    let api_version = iter.nth(1).unwrap();
    let rest = iter.nth(1).unwrap();
    format!("/{}/{}", api_version, without_extension(rest))
}

pub(crate) async fn compile_endpoints(sources: HashMap<String, String>) -> Result<()> {
    let promise = {
        let mut service = get();
        let service: &mut DenoService = &mut service;

        let mut handle = service.module_loader.lock().unwrap();
        let handle: &mut ModuleLoaderInner = &mut *handle;
        let code_map = &mut handle.code_map;

        let runtime = &mut service.worker.js_runtime;
        let scope = &mut runtime.handle_scope();
        let import_endpoints = service.import_endpoints.open(scope);
        let mut endpoints: Vec<v8::Local<'_, v8::Value>> = vec![];

        for (path, code) in sources {
            if let Ok(url) = Url::parse(&path) {
                // External module like https://deno.land/x/...
                // Insert as is.
                code_map.insert(url, code);
                continue;
            }
            if path.split('/').nth(2) != Some("endpoints") {
                // Non endpoint files like models/...
                let url = Url::parse(&format!("file://{}", path)).unwrap();
                code_map.insert(url, code);
                continue;
            }

            let path = without_extension(&path);
            let url = Url::parse(&format!("file://{}", path)).unwrap();
            code_map.insert(url, code);

            let path = endpoint_path_from_source_path(path);
            let path = RequestPath::try_from(path.as_ref()).unwrap();
            let api_version = v8::String::new(scope, path.api_version()).unwrap().into();
            let path = v8::String::new(scope, path.path()).unwrap().into();
            let endpoint = v8::Object::new(scope);
            let path_key = v8::String::new(scope, "path").unwrap();
            endpoint.set(scope, path_key.into(), path);
            let api_version_key = v8::String::new(scope, "apiVersion").unwrap();
            endpoint.set(scope, api_version_key.into(), api_version);
            endpoints.push(endpoint.into());
        }
        let endpoints = v8::Array::new_with_elements(scope, &endpoints).into();
        let undefined = v8::undefined(scope).into();
        let promise = import_endpoints
            .call(scope, undefined, &[endpoints])
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
