// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{anyhow, Context, Result};
use deno_core::anyhow;
use deno_core::op_sync;
use deno_core::serde;
use deno_core::url::Url;
use deno_core::v8;
use deno_core::JsRuntime;
use deno_core::OpFn;
use deno_core::RuntimeOptions;
use deno_core::Snapshot;
use deno_graph::source::LoadFuture;
use deno_graph::source::LoadResponse;
use deno_graph::source::Loader;
use deno_graph::Module;
use deno_graph::ModuleGraph;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug)]
struct DownloadMap {
    // map download path to urls and content
    path_to_url: HashMap<String, Url>,

    // maps url to the download path.
    url_to_path: HashMap<Url, String>,

    // Map a location (url or input file) to what it was compiled to.
    written: HashMap<String, String>,

    // maps absolute path without extension to the input as written.
    input_files: HashMap<String, String>,

    // User provided libraries
    extra_libs: HashMap<String, String>,

    // Precomputed module graph
    graph: ModuleGraph,

    diagnostics: String,
}

impl DownloadMap {
    fn len(&self) -> usize {
        self.path_to_url.len()
    }
    fn insert(&mut self, path: String, url: Url) {
        self.url_to_path.insert(url.clone(), path.clone());
        self.path_to_url.insert(path, url);
    }
    fn new(
        file_name: &str,
        extra_libs: HashMap<String, String>,
        graph: ModuleGraph,
    ) -> DownloadMap {
        let mut input_files = HashMap::new();
        input_files.insert(abs(without_extension(file_name)), file_name.to_string());
        DownloadMap {
            input_files,
            extra_libs,
            graph,
            path_to_url: Default::default(),
            url_to_path: Default::default(),
            written: Default::default(),
            diagnostics: Default::default(),
        }
    }
}

static MISSING_DEPENDENCY: &str = "/path/to/a/missing_dependency.d.ts";

fn fetch(map: &mut DownloadMap, path: String, mut base: String) -> Result<String> {
    if map.extra_libs.contains_key(&path) {
        return Ok(path);
    }
    if let Some(url) = map.path_to_url.get(&base) {
        base = url.to_string();
    } else {
        assert!(base.as_bytes()[0] == b'/');
        base = "file://".to_string() + &base;
    }
    let resolved = match deno_core::resolve_import(&path, &base) {
        Ok(v) => v,
        Err(_) => {
            return Ok(MISSING_DEPENDENCY.to_string());
        }
    };
    if let Some(path) = map.url_to_path.get(&resolved) {
        return Ok(path.clone());
    }

    let n = map.len();
    let extension = if path.ends_with(".d.ts") {
        "d.ts"
    } else if path.ends_with(".js") {
        "js"
    } else {
        "ts"
    };

    let path = format!("/path/to/downloaded/files/{}.{}", n, extension);
    map.insert(path.clone(), resolved);
    Ok(path)
}

fn op<T1, T2, R, F>(func: F) -> Box<OpFn>
where
    T1: DeserializeOwned,
    T2: DeserializeOwned,
    R: Serialize + 'static,
    F: Fn(&mut DownloadMap, T1, T2) -> Result<R> + 'static,
{
    op_sync(move |s, a1, a2| {
        let map = s.borrow_mut::<DownloadMap>();
        func(map, a1, a2)
    })
}

fn read(map: &mut DownloadMap, path: String, _: ()) -> Result<String> {
    if path == MISSING_DEPENDENCY {
        return Ok("export default function(): unknown;\n".to_string());
    }
    if let Some(v) = map.extra_libs.get(&path) {
        return Ok(v.to_string());
    }
    if let Some(url) = map.path_to_url.get(&path) {
        let module = match map.graph.get(url).unwrap() {
            Module::Es(m) => m,
            Module::Synthetic(_) => anyhow::bail!("Was not expecting a synthetic module"),
        };
        return Ok((*module.source).clone());
    }
    fs::read_to_string(&path).with_context(|| format!("Reading {}", path))
}

fn write(map: &mut DownloadMap, mut path: String, content: String) -> Result<()> {
    path = path.strip_prefix("chisel:/").unwrap().to_string();
    if let Some(url) = map.path_to_url.get(&path) {
        path = url.to_string();
    } else {
        let (prefix, is_dts) = match path.strip_suffix(".d.ts") {
            None => (without_extension(&path), false),
            Some(prefix) => (prefix, true),
        };
        path = match map.input_files.get(prefix) {
            Some(path) => path.clone(),
            None => return Ok(()),
        };
        if is_dts {
            path = without_extension(&path).to_string() + ".d.ts";
        }
    }
    map.written.insert(path, content);
    Ok(())
}

fn get_cwd(_map: &mut DownloadMap, _: (), _: ()) -> Result<String> {
    let cwd = std::env::current_dir()?;
    Ok(cwd.into_os_string().into_string().unwrap())
}

fn dir_exists(_map: &mut DownloadMap, path: String, _: ()) -> Result<bool> {
    return Ok(Path::new(&path).is_dir());
}

fn file_exists(_map: &mut DownloadMap, path: String, _: ()) -> Result<bool> {
    return Ok(Path::new(&path).is_file());
}

fn diagnostic(map: &mut DownloadMap, msg: String, _: ()) -> Result<()> {
    map.diagnostics = msg;
    Ok(())
}

fn try_into_or<'s, T: std::convert::TryFrom<v8::Local<'s, v8::Value>>>(
    val: Option<v8::Local<'s, v8::Value>>,
) -> Result<T>
where
    T::Error: std::error::Error + Send + Sync + 'static,
{
    Ok(val.ok_or(anyhow!("None"))?.try_into()?)
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
    let val = obj.get(scope, key.into());
    let res: T = try_into_or(val)?;
    Ok(res)
}

// Paths are passed to javascript, which uses UTF-16, no point in
// pretending we can handle non unicode PathBufs.
fn abs(path: &str) -> String {
    let mut p = env::current_dir().unwrap();
    p.push(path);
    p.into_os_string().into_string().unwrap()
}

fn without_extension(path: &str) -> &str {
    path.rsplit_once('.').map_or(path, |p| p.0)
}

static SNAPSHOT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/SNAPSHOT.bin"));

#[derive(Default, Clone)]
pub struct CompileOptions<'a> {
    pub extra_default_lib: Option<&'a str>,
    pub extra_libs: HashMap<String, String>,
    pub emit_declarations: bool,
}

struct ModuleLoader {}

async fn load_url(specifier: Url) -> Result<Option<LoadResponse>> {
    let mut maybe_headers = None;
    let text = if specifier.scheme() == "file" {
        fs::read_to_string(specifier.to_file_path().unwrap())?
    } else {
        let res = reqwest::get(specifier.clone()).await?;
        let mut headers = HashMap::new();
        for (key, value) in res.headers().iter() {
            headers.insert(key.as_str().to_string(), value.to_str()?.to_string());
        }
        maybe_headers = Some(headers);
        res.text().await?
    };
    let response = LoadResponse {
        specifier,
        maybe_headers,
        content: Arc::new(text),
    };
    Ok(Some(response))
}

impl Loader for ModuleLoader {
    fn load(&mut self, specifier: &Url, _is_dynamic: bool) -> LoadFuture {
        Box::pin(load_url(specifier.clone()))
    }
}

pub async fn compile_ts_code(
    file_name: &str,
    opts: CompileOptions<'_>,
) -> Result<HashMap<String, String>> {
    let url = "file://".to_string() + &abs(file_name);
    let url = Url::parse(&url)?;
    let mut loader = ModuleLoader {};
    let graph =
        deno_graph::create_graph(vec![url], false, None, &mut loader, None, None, None, None).await;

    let mut runtime = JsRuntime::new(RuntimeOptions {
        startup_snapshot: Some(Snapshot::Static(SNAPSHOT)),
        ..Default::default()
    });
    runtime.register_op("fetch", op(fetch));
    runtime.register_op("read", op(read));
    runtime.register_op("write", op(write));
    runtime.register_op("get_cwd", op(get_cwd));
    runtime.register_op("dir_exists", op(dir_exists));
    runtime.register_op("file_exists", op(file_exists));
    runtime.register_op("diagnostic", op(diagnostic));
    runtime.sync_ops_cache();

    runtime
        .op_state()
        .borrow_mut()
        .put(DownloadMap::new(file_name, opts.extra_libs, graph));

    let global_context = runtime.global_context();
    let ok = {
        let scope = &mut runtime.handle_scope();
        let file = v8::String::new(scope, &abs(file_name)).unwrap().into();
        let lib = match opts.extra_default_lib {
            Some(v) => v8::String::new(scope, &abs(v)).unwrap().into(),
            None => v8::undefined(scope).into(),
        };
        let global_proxy = global_context.open(scope).global(scope);
        let compile: v8::Local<v8::Function> = get_member(global_proxy, scope, "compile").unwrap();
        let emit_declarations = v8::Boolean::new(scope, opts.emit_declarations).into();
        compile
            .call(scope, global_proxy.into(), &[file, lib, emit_declarations])
            .unwrap()
            .is_true()
    };

    let op_state = runtime.op_state();
    let op_state = op_state.borrow();
    let map = op_state.borrow::<DownloadMap>();
    if ok {
        Ok(map.written.clone())
    } else {
        Err(anyhow!("Compilation failed:\n{}", map.diagnostics))
    }
}

#[cfg(test)]
mod tests {
    use super::compile_ts_code;
    use super::CompileOptions;
    use anyhow::Result;
    use deno_core::anyhow;
    use std::io::Write;
    use tempfile::Builder;
    use tempfile::NamedTempFile;

    struct TestTemp(NamedTempFile);

    fn write_temp(text: &[u8]) -> Result<TestTemp> {
        let mut f = Builder::new().suffix(".ts").tempfile()?;
        f.write_all(text)?;
        Ok(TestTemp(f))
    }

    impl TestTemp {
        fn path(&self) -> &str {
            self.0.path().to_str().unwrap()
        }
    }

    #[tokio::test]
    async fn test() -> Result<()> {
        let f = write_temp(b"import * as zed from \"@foo/bar\";")?;
        let libs = [("@foo/bar".to_string(), "export {}".to_string())]
            .into_iter()
            .collect();
        let opts = CompileOptions {
            extra_libs: libs,
            ..Default::default()
        };
        compile_ts_code(f.path(), opts).await?;
        Ok(())
    }

    #[tokio::test]
    async fn diagnostics() -> Result<()> {
        for _ in 0..2 {
            let f = write_temp(b"export {}; zed;")?;
            let err = compile_ts_code(f.path(), Default::default()).await;
            let err = err.unwrap_err().to_string();
            assert!(err.contains("Cannot find name 'zed'"));
        }
        Ok(())
    }

    #[tokio::test]
    async fn property_constructor_not_strict() -> Result<()> {
        let f = write_temp(b"export class Foo { a: number };")?;
        compile_ts_code(f.path(), Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn missing_file() -> Result<()> {
        let err = compile_ts_code("/no/such/file.ts", Default::default())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("Cannot read file '/no/such/file.ts': Reading /no/such/file.ts."));
        Ok(())
    }

    #[tokio::test]
    async fn no_extension() -> Result<()> {
        let err = compile_ts_code("/no/such/file", Default::default())
            .await
            .unwrap_err()
            .to_string();
        // FIXME: Something is adding a .ts extension
        assert!(err.contains("Cannot read file '/no/such/file.ts': Reading /no/such/file.ts."));
        Ok(())
    }

    #[tokio::test]
    async fn async_iter_readable_stream() -> Result<()> {
        let f = write_temp(
            br#"
export function foo<R>(bar: ReadableStream<R>): AsyncIterableIterator<R> {
    return bar[Symbol.asyncIterator]();
}
"#,
        )?;
        compile_ts_code(f.path(), Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn fails_with_emit() -> Result<()> {
        let f = write_temp(
            br#"
var foo = function() {};
foo.prototype = {};
foo.prototype.bar = foo;
export default foo;
"#,
        )?;
        compile_ts_code(f.path(), Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn bad_import() -> Result<()> {
        let mut f = Builder::new()
            .prefix("bad_import")
            .suffix(".ts")
            .tempfile()?;
        f.write_all(b"import {foo} from \"bar\";")?;
        let path = f.path().to_str().unwrap();
        let err = compile_ts_code(path, Default::default())
            .await
            .unwrap_err()
            .to_string();
        // Unfortunately we don't report errors on module resolution,
        // so we only get an error about there being no 'foo' in
        // 'bar'.
        assert!(err.contains("Module '\"bar\"' has no exported member 'foo'"));
        Ok(())
    }

    #[tokio::test]
    async fn handlebars() -> Result<()> {
        let f = write_temp(b"import handlebars from \"https://cdn.skypack.dev/handlebars\";")?;
        compile_ts_code(f.path(), Default::default()).await?;
        Ok(())
    }
}
