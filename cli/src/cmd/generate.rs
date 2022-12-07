use crate::proto::chisel_rpc_client::ChiselRpcClient;
use crate::proto::{type_msg::TypeEnum, DescribeRequest};
use crate::proto::{FieldDefinition, TypeDefinition, TypeMsg, VersionDefinition};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use tokio::fs::create_dir_all;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Node,
    Deno,
}

#[derive(Debug)]
pub struct Opts {
    pub server_url: String,
    pub api_addres: String,
    pub output_dir: PathBuf,
    pub version: String,
    pub mode: Mode,
}

pub(crate) async fn cmd_generate(opts: Opts) -> Result<()> {
    create_dir_all(&opts.output_dir)
        .await
        .context("failed to create directory for generated client files")?;
    let version_def = fetch_version_def(&opts).await?;

    let mut models_file = File::create(opts.output_dir.join("models.ts"))?;
    write_models(&mut models_file, &version_def)?;

    let mut reflection_file = File::create(opts.output_dir.join("reflection.ts"))?;
    write_reflection(&mut reflection_file, &version_def)?;

    let routes = get_routing_info(&opts.api_addres, &opts.version).await?;
    let mut client_file = File::create(opts.output_dir.join("client.ts"))?;
    write_routing_client(&mut client_file, &routes, &opts)?;

    let mut client_lib_file = File::create(opts.output_dir.join("client_lib.ts"))?;
    write_client_lib(&mut client_lib_file, &opts)?;

    let mut filter_file = File::create(opts.output_dir.join("filter.ts"))?;
    write!(
        filter_file,
        "{}",
        include_str!("../../../api/src/filter.ts")
    )?;

    Ok(())
}

async fn fetch_version_def(opts: &Opts) -> Result<VersionDefinition> {
    let mut client = ChiselRpcClient::connect(opts.server_url.to_owned()).await?;
    let request = tonic::Request::new(DescribeRequest {});
    let response = execute!(client.describe(request).await);
    let version_def = response
        .version_defs
        .iter()
        .find(|def| def.version_id == opts.version)
        .context(anyhow!(
            "can't generate client with an unknown version '{:?}'",
            opts.version
        ))?;
    Ok(version_def.clone())
}

fn write_models(output: &mut File, version_def: &VersionDefinition) -> Result<()> {
    for def in &version_def.type_defs {
        writeln!(output, "export type {} = {{", def.name)?;
        writeln!(output, "    id: string;")?;
        for field in &def.field_defs {
            let field_type = field.field_type()?;
            writeln!(
                output,
                "    {}{}: {};",
                field.name,
                if field.is_optional { "?" } else { "" },
                field_type
            )?;
        }
        writeln!(output, "}}")?;
    }
    Ok(())
}

fn write_reflection(output: &mut File, version_def: &VersionDefinition) -> Result<()> {
    write!(output, "{}", include_str!("generate_src/reflection.ts"))?;

    let entites: HashMap<String, TypeDefinition> = HashMap::from_iter(
        version_def
            .type_defs
            .iter()
            .map(|ty| (ty.name.to_owned(), ty.clone())),
    );

    for entity_name in entites.keys() {
        writeln!(
            output,
            "export const Ω{}: Entity = {}",
            entity_name,
            make_entity_type_obj(&entites, entity_name)?
        )?;
    }
    Ok(())
}

fn make_entity_type_obj(
    entities: &HashMap<String, TypeDefinition>,
    entity_name: &str,
) -> Result<serde_json::Value> {
    let entity = entities.get(entity_name).context(anyhow!(
        "trying to generate entity object from an unknown entity name '{entity_name:?}'"
    ))?;

    let fields: Vec<_> = entity
        .field_defs
        .iter()
        .map(|field| make_field_obj(entities, field))
        .collect::<Result<_>>()?;

    Ok(json!({
        "name": entity_name,
        "fields": fields
    }))
}

fn make_field_obj(
    entities: &HashMap<String, TypeDefinition>,
    field: &FieldDefinition,
) -> Result<serde_json::Value> {
    let field_type = field
        .field_type
        .as_ref()
        .context("field doesn't have type")?;
    let type_obj = type_to_obj(entities, field_type)?;
    Ok(json!({
        "name": field.name,
        "type": type_obj,
        "isOptional": field.is_optional,
        "isUnique": field.is_unique
    }))
}

fn type_to_obj(
    entities: &HashMap<String, TypeDefinition>,
    ty: &TypeMsg,
) -> Result<serde_json::Value> {
    let type_enum = ty
        .type_enum
        .as_ref()
        .context("field doesn't have type_enum")?;
    let type_ojbect = match &type_enum {
        TypeEnum::ArrayBuffer(_) => json!({"name": "arrayBuffer"}),
        TypeEnum::Bool(_) => json!({"name": "boolean"}),
        TypeEnum::JsDate(_) => json!({"name": "date"}),
        TypeEnum::Number(_) => json!({"name": "number"}),
        TypeEnum::String(_) => json!({"name": "string"}),
        TypeEnum::EntityId(entity_name) => json!({
            "name": "entityId",
            "entityName": entity_name
        }),
        TypeEnum::Array(container) => {
            let element_type = &container
                .value_type
                .as_ref()
                .context("container has no value")?;
            json!({
                "name": "array",
                "elementType": type_to_obj(entities, element_type)?
            })
        }
        TypeEnum::Entity(entity_name) => {
            json!({
                "name": "entity",
                "entityType": make_entity_type_obj(entities, entity_name)?
            })
        }
    };
    Ok(type_ojbect)
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all(deserialize = "UPPERCASE"))]
enum HttpMethod {
    Options,
    Get,
    Post,
    Put,
    Delete,
    Head,
    Trace,
    Connect,
    Patch,
}

#[derive(Debug, Deserialize)]
struct ClientMetadata {
    handler: HandlerKind,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouteInfo {
    methods: Vec<HttpMethod>,
    path_pattern: String,
    client_metadata: Option<ClientMetadata>,
}

async fn get_routing_info(api_listen_addr: &str, version: &str) -> Result<Vec<RouteInfo>> {
    let chisel_url = reqwest::Url::parse(&format!("http://{}", api_listen_addr))?;
    let url = chisel_url.join(&format!("/{version}/__chiselstrike/routes"))?;
    Ok(reqwest::get(url).await?.json().await?)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", content = "handler")]
enum HandlerKind {
    Crud(CrudHandler),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", content = "entityName")]
enum CrudHandler {
    GetOne(String),
    GetMany(String),
    PutOne(String),
    PostOne(String),
    PatchOne(String),
    DeleteOne(String),
    DeleteMany(String),
}

#[derive(Debug)]
struct RouteHandler {
    _method: HttpMethod,
    kind: HandlerKind,
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
enum RouteSegment {
    FixedText(String),
    Wildcard(String),
}

#[derive(Debug)]
struct SubRoute {
    handlers: Vec<RouteHandler>,
    children: HashMap<RouteSegment, SubRoute>,
}

impl SubRoute {
    fn new() -> Self {
        Self {
            handlers: vec![],
            children: Default::default(),
        }
    }
    fn add_route(&mut self, route: &str, handler: RouteHandler) {
        let route_segments: Vec<_> = route
            .trim()
            .trim_start_matches('/')
            .trim_end_matches('/')
            .split('/')
            .collect();
        self.add_route_rec(&route_segments, handler);
    }

    fn add_route_rec(&mut self, segments: &[&str], handler: RouteHandler) {
        if segments.is_empty() {
            self.handlers.push(handler);
        } else {
            let segment_str = segments[0];
            let sub_segments = &segments[1..];
            let segment = Self::make_segment(segment_str);

            if let Some(child) = self.children.get_mut(&segment) {
                child.add_route_rec(sub_segments, handler);
            } else {
                let mut subroute = SubRoute::new();
                subroute.add_route_rec(sub_segments, handler);
                self.children.insert(segment, subroute);
            }
        }
    }

    fn make_segment(segment_str: &str) -> RouteSegment {
        if segment_str.starts_with(':') {
            RouteSegment::Wildcard(segment_str.to_owned())
        } else {
            RouteSegment::FixedText(segment_str.to_owned())
        }
    }
}

fn path_join(p1: &str, p2: &str) -> String {
    format!(
        "{}/{}",
        p1.trim_end_matches('/'),
        p2.trim_start_matches('/')
    )
}

fn build_routing(routes: &Vec<RouteInfo>) -> Result<SubRoute> {
    let mut root = SubRoute::new();

    for route in routes {
        let methods = &route.methods;
        if let Some(meta) = &route.client_metadata {
            anyhow::ensure!(
                methods.len() == 1,
                "the number of allowed route methods must be one"
            );
            root.add_route(
                &route.path_pattern,
                RouteHandler {
                    _method: methods[0].clone(),
                    kind: meta.handler.clone(),
                },
            );
        }
    }
    Ok(root)
}

fn handler_to_ts(handler: &RouteHandler, url: &str) -> Vec<String> {
    let HandlerKind::Crud(crud_handler) = &handler.kind;
    match &crud_handler {
        CrudHandler::DeleteMany(entity_name) => {
            vec![format!(
                "delete: Ωlib.makeDeleteMany<Ωmodels.{entity_name}>(Ωurl(`{url}`))"
            )]
        }
        CrudHandler::DeleteOne(_) => vec![format!("delete: Ωlib.makeDeleteOne(Ωurl(`{url}`))")],
        CrudHandler::GetMany(entity_name) => {
            vec![format!(
                "get: Ωlib.makeGetMany<Ωmodels.{entity_name}>(Ωurl(`{url}`), ΩserverUrl, Ωreflection.Ω{entity_name})"
            ), format!(
                "getIter: Ωlib.makeGetManyIter<Ωmodels.{entity_name}>(Ωurl(`{url}`), ΩserverUrl, Ωreflection.Ω{entity_name})"
            ), format!(
                "getAll: Ωlib.makeGetAll<Ωmodels.{entity_name}>(Ωurl(`{url}`), ΩserverUrl, Ωreflection.Ω{entity_name})"
            )]
        }
        CrudHandler::GetOne(entity_name) => {
            vec![format!(
                "get: Ωlib.makeGetOne<Ωmodels.{entity_name}>(Ωurl(`{url}`), Ωreflection.Ω{entity_name})"
            )]
        }
        CrudHandler::PatchOne(entity_name) => {
            vec![format!(
                "patch: Ωlib.makePatchOne<Ωmodels.{entity_name}>(Ωurl(`{url}`), Ωreflection.Ω{entity_name})"
            )]
        }
        CrudHandler::PostOne(entity_name) => {
            vec![format!(
                "post: Ωlib.makePostOne<Ωmodels.{entity_name}>(Ωurl(`{url}`), Ωreflection.Ω{entity_name})"
            )]
        }
        CrudHandler::PutOne(entity_name) => {
            vec![format!(
                "put: Ωlib.makePutOne<Ωmodels.{entity_name}>(Ωurl(`{url}`), Ωreflection.Ω{entity_name})"
            )]
        }
    }
}

fn generate_routing_obj(route: &SubRoute, url_prefix: &str) -> Result<String> {
    let mut handlers: Vec<_> = route
        .handlers
        .iter()
        .flat_map(|h| handler_to_ts(h, url_prefix))
        .collect();

    for (segment, subroute) in &route.children {
        let handler = match segment {
            RouteSegment::FixedText(segment) => format!(
                "{segment}: {}",
                generate_routing_obj(subroute, &path_join(url_prefix, segment))?
            ),
            RouteSegment::Wildcard(segment) => {
                let group_name = segment.trim_start_matches(':');
                let url_path = path_join(url_prefix, &format!("${{{group_name}}}"));
                format!(
                    "{group_name}: ({group_name}: string) => {{ return {}; }}",
                    generate_routing_obj(subroute, &url_path)?
                )
            }
        };
        handlers.push(handler);
    }
    Ok(format!("{{{}}}\n", handlers.join(",\n")))
}

fn write_routing_client(output: &mut File, routes: &Vec<RouteInfo>, opts: &Opts) -> Result<()> {
    let imports = match opts.mode {
        Mode::Deno => {
            r#"
                import * as Ωlib from "./client_lib.ts";
                import * as Ωmodels from "./models.ts";
                import * as Ωreflection from "./reflection.ts";
            "#
        }
        Mode::Node => {
            r#"
                import * as Ωlib from "./client_lib";
                import * as Ωmodels from "./models";
                import * as Ωreflection from "./reflection";
            "#
        }
    };
    write!(output, "{}", &imports)?;
    write!(output, "{}", include_str!("generate_src/client.ts"))?;
    let root = build_routing(routes)?;

    writeln!(
        output,
        r#"
            function ΩcreateClient(ΩserverUrl: string, ΩuserVersion?: string) {{
                const Ωversion = ΩuserVersion ?? '{}';
                const Ωurl = (url: string) => {{
                    return Ωlib.urlJoin(ΩserverUrl, Ωversion, url);
                }};
                return {};
            }}
        "#,
        opts.version,
        generate_routing_obj(&root, "")?
    )?;

    Ok(())
}

fn write_client_lib(output: &mut File, opts: &Opts) -> Result<()> {
    let imports = match opts.mode {
        Mode::Deno => {
            r#"
            import { type FilterExpr } from "./filter.ts";
            import * as reflect from "./reflection.ts";
        "#
        }
        Mode::Node => {
            r#"
            import { type FilterExpr } from "./filter";
            import * as reflect from "./reflection";
        "#
        }
    };
    write!(output, "{}", &imports)?;
    write!(output, "{}", include_str!("generate_src/client_lib.ts"))?;
    Ok(())
}
