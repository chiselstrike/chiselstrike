use guard::guard;
use std::collections::HashMap;

#[derive(Debug)]
pub struct SourceRow {
    pub path: String,
    pub code: String,
}

#[derive(Debug)]
pub struct ModuleRow {
    pub version_id: String,
    pub url: String,
    pub code: String,
}

#[derive(Debug)]
struct VersionSourceMap {
    version_id: String,
    endpoints: Vec<EndpointSource>,
    others: Vec<Source>,
}

#[derive(Debug)]
struct EndpointSource {
    route: String,
    source: Source,
}

#[derive(Debug)]
struct Source {
    path: String,
    code: String,
}

#[derive(Debug)]
struct SourcePath {
    version_id: String,
    path: String,
    endpoint_route: Option<String>,
}

pub fn migrate_sources(source_rows: Vec<SourceRow>) -> Vec<ModuleRow> {
    let mut source_maps = HashMap::new();
    for row in source_rows.into_iter() {
        guard! {let Some(source_path) = parse_source_path(&row.path) else {
            continue
        }};

        let source = Source {
            path: source_path.path,
            code: row.code,
        };

        let source_map = source_maps
            .entry(source_path.version_id.clone())
            .or_insert_with(|| VersionSourceMap {
                version_id: source_path.version_id,
                endpoints: Vec::new(),
                others: Vec::new(),
            });

        if let Some(route) = source_path.endpoint_route {
            source_map.endpoints.push(EndpointSource { route, source });
        } else {
            source_map.others.push(source);
        }
    }

    let mut module_rows = Vec::new();
    for source_map in source_maps.into_values() {
        migrate_version(source_map, &mut module_rows);
    }
    module_rows
}

fn parse_source_path(path: &str) -> Option<SourcePath> {
    let path = path.trim_start_matches('/').to_owned();
    let (version_id, rel_path) = path.split_once('/')?;
    let endpoint_route = match rel_path.split_once('/') {
        Some(("endpoints" | "routes", endpoint_rel_path)) => Some(
            endpoint_rel_path
                .trim_end_matches(".ts")
                .trim_end_matches(".js")
                .into(),
        ),
        _ => None,
    };
    Some(SourcePath {
        version_id: version_id.into(),
        path,
        endpoint_route,
    })
}

fn migrate_version(source_map: VersionSourceMap, module_rows: &mut Vec<ModuleRow>) {
    module_rows.push(codegen_root(&source_map.version_id, &source_map.endpoints));
    for endpoint in source_map.endpoints.into_iter() {
        module_rows.push(migrate_source(&source_map.version_id, endpoint.source));
    }
    for source in source_map.others.into_iter() {
        module_rows.push(migrate_source(&source_map.version_id, source));
    }
}

fn codegen_root(version_id: &str, endpoints: &[EndpointSource]) -> ModuleRow {
    let mut lines = Vec::new();
    lines.push(format!("// this code was migrated by {}", file!()));
    lines.push("import { RouteMap } from 'chisel://api/routing.ts';".into());
    lines.push("import { TopicMap } from 'chisel://api/kafka.ts';".into());
    lines.push("export const routeMap = new RouteMap();".into());
    lines.push("export const topicMap = new TopicMap();".into());
    lines.push("".into());

    for (i, endpoint) in endpoints.iter().enumerate() {
        let import_url = path_to_url(&endpoint.source.path);
        // TODO: we quote strings using fmt::Debug, but we should really quote them as JavaScript
        // strings
        lines.push(format!("import endpoint{} from {:?};", i, import_url));
        lines.push(format!(
            "routeMap.prefix({:?}, RouteMap.convert(endpoint{}, {:?}));",
            endpoint.route, i, endpoint.route,
        ));
    }

    ModuleRow {
        version_id: version_id.into(),
        url: "file:///__root.ts".into(),
        code: lines.join("\n"),
    }
}

fn migrate_source(version_id: &str, source: Source) -> ModuleRow {
    ModuleRow {
        version_id: version_id.into(),
        url: path_to_url(&source.path),
        code: source.code,
    }
}

fn path_to_url(path: &str) -> String {
    format!("file:///{}", path)
}
