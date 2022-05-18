// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{anyhow, Context, Result};
use deno_core::anyhow;
use deno_core::op;
use deno_core::serde;
use deno_core::url::Url;
use deno_core::v8;
use deno_core::Extension;
use deno_core::JsRuntime;
use deno_core::OpState;
use deno_core::RuntimeOptions;
use deno_core::Snapshot;
use deno_graph::resolve_import;
use deno_graph::source::LoadFuture;
use deno_graph::source::LoadResponse;
use deno_graph::source::LoadResult;
use deno_graph::source::Loader;
use deno_graph::source::ResolveResponse;
use deno_graph::source::Resolver;
use deno_graph::ModuleGraph;
use deno_graph::ModuleKind;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::future::Future;
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

fn fetch_impl(map: &mut DownloadMap, path: String, mut base: String) -> Result<String> {
    if map.extra_libs.contains_key(&path) {
        return Ok(path);
    }
    if let Some(url) = map.path_to_url.get(&base) {
        base = url.to_string();
    } else {
        assert!(base.as_bytes()[0] == b'/');
        base = "file://".to_string() + &base;
    }
    let url = Url::parse(&base).unwrap();
    let resolved = map
        .graph
        .resolve_dependency(&path, &url, true)
        .ok_or_else(|| anyhow!("Could not resolve '{}' in '{}'", path, url))?
        .clone();
    if let Some(path) = map.url_to_path.get(&resolved) {
        return Ok(path.clone());
    }

    let n = map.len();
    let module = map.graph.get(&resolved).unwrap();
    let extension = module.media_type.as_ts_extension();
    let path = format!("/path/to/downloaded/files/{}.{}", n, extension);
    map.insert(path.clone(), resolved);
    Ok(path)
}

fn with_map<T1, T2, R, F>(func: F, s: &mut OpState, a: T1, b: T2) -> Result<R>
where
    T1: DeserializeOwned,
    T2: DeserializeOwned,
    R: Serialize + 'static,
    F: Fn(&mut DownloadMap, T1, T2) -> Result<R>,
{
    let map = s.borrow_mut::<DownloadMap>();
    func(map, a, b)
}

#[op]
fn fetch(s: &mut OpState, path: String, base: String) -> Result<String> {
    with_map(fetch_impl, s, path, base)
}

fn read_impl(map: &mut DownloadMap, path: String, _: ()) -> Result<String> {
    if let Some(v) = map.extra_libs.get(&path) {
        return Ok(v.to_string());
    }

    let url = if let Some(url) = map.path_to_url.get(&path) {
        url.clone()
    } else {
        let url = "file://".to_string() + &path;
        Url::parse(&url)?
    };
    let module = match map.graph.try_get(&url) {
        Ok(Some(m)) => m,
        Ok(None) => anyhow::bail!("URL was not loaded"),
        Err(e) => return Err(e.into()),
    };
    Ok((**module.maybe_source.as_ref().unwrap()).clone())
}

#[op]
fn read(s: &mut OpState, path: String) -> Result<String> {
    with_map(read_impl, s, path, ())
}

fn write_impl(map: &mut DownloadMap, mut path: String, content: String) -> Result<()> {
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

#[op]
fn write(s: &mut OpState, path: String, content: String) -> Result<()> {
    with_map(write_impl, s, path, content)
}

#[op]
fn get_cwd() -> Result<String> {
    let cwd = std::env::current_dir()?;
    Ok(cwd.into_os_string().into_string().unwrap())
}

#[op]
fn dir_exists(path: String) -> Result<bool> {
    return Ok(Path::new(&path).is_dir());
}

#[op]
fn file_exists(path: String) -> Result<bool> {
    return Ok(Path::new(&path).is_file());
}

fn diagnostic_impl(map: &mut DownloadMap, msg: String, _: ()) -> Result<()> {
    map.diagnostics = msg;
    Ok(())
}

#[op]
fn diagnostic(s: &mut OpState, msg: String) -> Result<()> {
    with_map(diagnostic_impl, s, msg, ())
}

fn try_into_or<'s, T: std::convert::TryFrom<v8::Local<'s, v8::Value>>>(
    val: Option<v8::Local<'s, v8::Value>>,
) -> Result<T>
where
    T::Error: std::error::Error + Send + Sync + 'static,
{
    Ok(val.context("None")?.try_into()?)
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

struct ModuleLoader {
    extra_libs: HashMap<Url, String>,
}

fn load_url(extra_libs: &HashMap<Url, String>, specifier: Url) -> impl Future<Output = LoadResult> {
    let sync_text = match specifier.scheme() {
        "file" => fs::read_to_string(specifier.to_file_path().unwrap()),
        "chisel" => Ok(extra_libs.get(&specifier).unwrap().clone()),
        _ => Ok("".to_string()),
    };
    let mut maybe_headers = None;

    async {
        let text = match specifier.scheme() {
            "file" | "chisel" => sync_text?,
            _ => {
                let res = utils::get_ok(specifier.clone()).await?;
                let mut headers = HashMap::new();
                for (key, value) in res.headers().iter() {
                    headers.insert(key.as_str().to_string(), value.to_str()?.to_string());
                }
                maybe_headers = Some(headers);
                res.text().await?
            }
        };
        let response = LoadResponse::Module {
            specifier,
            maybe_headers,
            content: Arc::new(text),
        };
        Ok(Some(response))
    }
}

impl Loader for ModuleLoader {
    fn load(&mut self, specifier: &Url, _is_dynamic: bool) -> LoadFuture {
        Box::pin(load_url(&self.extra_libs, specifier.clone()))
    }
}

#[derive(Debug)]
struct ModuleResolver {
    extra_libs: HashMap<String, Url>,
}

impl Resolver for ModuleResolver {
    fn resolve(&self, specifier: &str, referrer: &Url) -> ResolveResponse {
        if let Some(u) = self.extra_libs.get(specifier) {
            return ResolveResponse::Esm(u.clone());
        }
        resolve_import(specifier, referrer).into()
    }
}

pub struct Compiler {
    pub runtime: JsRuntime,
}

impl Default for Compiler {
    fn default() -> Compiler {
        let ext = Extension::builder()
            .ops(vec![
                fetch::decl(),
                read::decl(),
                write::decl(),
                get_cwd::decl(),
                dir_exists::decl(),
                file_exists::decl(),
                diagnostic::decl(),
            ])
            .build();

        let runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![ext],
            startup_snapshot: Some(Snapshot::Static(SNAPSHOT)),
            ..Default::default()
        });

        Compiler { runtime }
    }
}

impl Compiler {
    pub async fn compile_ts_code(
        &mut self,
        file_name: &str,
        opts: CompileOptions<'_>,
    ) -> Result<HashMap<String, String>> {
        let url = "file://".to_string() + &abs(file_name);
        let url = Url::parse(&url)?;

        let mut extra_libs = HashMap::new();
        let mut to_url = HashMap::new();
        for (k, v) in &opts.extra_libs {
            let u = Url::parse(&("chisel:///".to_string() + k + ".ts")).unwrap();
            extra_libs.insert(u.clone(), v.clone());
            to_url.insert(k.clone(), u);
        }

        let mut loader = ModuleLoader { extra_libs };
        let resolver = ModuleResolver { extra_libs: to_url };

        let maybe_imports = if let Some(path) = opts.extra_default_lib {
            let dummy_url = Url::parse("chisel://std").unwrap();
            let path = "file://".to_string() + &abs(path);
            Some(vec![(dummy_url, vec![path])])
        } else {
            None
        };

        let graph = deno_graph::create_graph(
            vec![(url, ModuleKind::Esm)],
            false,
            maybe_imports,
            &mut loader,
            Some(&resolver),
            None,
            None,
            None,
        )
        .await;

        graph.valid()?;

        self.runtime.op_state().borrow_mut().put(DownloadMap::new(
            file_name,
            opts.extra_libs,
            graph,
        ));

        let global_context = self.runtime.global_context();
        let ok = {
            let scope = &mut self.runtime.handle_scope();
            let file = v8::String::new(scope, &abs(file_name)).unwrap().into();
            let lib = match opts.extra_default_lib {
                Some(v) => v8::String::new(scope, &abs(v)).unwrap().into(),
                None => v8::undefined(scope).into(),
            };
            let global_proxy = global_context.open(scope).global(scope);
            let compile: v8::Local<v8::Function> =
                get_member(global_proxy, scope, "compile").unwrap();
            let emit_declarations = v8::Boolean::new(scope, opts.emit_declarations).into();
            compile
                .call(scope, global_proxy.into(), &[file, lib, emit_declarations])
                .unwrap()
                .is_true()
        };

        let op_state = self.runtime.op_state();
        let op_state = op_state.borrow();
        let map = op_state.borrow::<DownloadMap>();
        if ok {
            Ok(map.written.clone())
        } else {
            Err(anyhow!("Compilation failed:\n{}", map.diagnostics))
        }
    }
}

pub async fn compile_ts_code(
    file_name: &str,
    opts: CompileOptions<'_>,
) -> Result<HashMap<String, String>> {
    let mut compiler = Compiler::default();
    compiler.compile_ts_code(file_name, opts).await
}

#[cfg(test)]
mod tests {
    use super::abs;
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

    async fn check_test_ts(path: &str) {
        let opts = CompileOptions {
            emit_declarations: true,
            ..Default::default()
        };
        let written = compile_ts_code(path, opts).await.unwrap();
        let mut keys: Vec<_> = written.keys().collect();
        keys.sort();

        let dts = path.strip_suffix(".ts").unwrap().to_string() + ".d.ts";
        let mut expected = vec![
            "https://cdn.skypack.dev/-/indent-string@v5.0.0-VgKPSgi4hUX5NbF4n3aC/dist=es2020,mode=imports,min/optimized/indent-string.js",
            "https://cdn.skypack.dev/pin/indent-string@v5.0.0-VgKPSgi4hUX5NbF4n3aC/mode=imports,min/optimized/indent-string.js",
            path,
            &dts,
        ];
        expected.sort_unstable();
        assert_eq!(keys, expected);
        let body = written[path].clone();
        assert!(body.starts_with("import indent from"));
        assert!(!body.contains("undefined"));
    }

    async fn check_test2_ts(path: &str) {
        let written = compile_ts_code(path, Default::default()).await.unwrap();
        let body = written[path].clone();
        assert!(body.starts_with("import { zed } from"));
    }

    #[tokio::test]
    async fn test1() {
        let p = "tests/test1.ts";
        check_test_ts(p).await;
        check_test_ts(&abs(p)).await;
    }

    #[tokio::test]
    async fn test2() {
        let p = "tests/test2.ts";
        check_test2_ts(p).await;
        check_test2_ts(&abs(p)).await;
    }

    #[tokio::test]
    async fn test3() -> Result<()> {
        compile_ts_code("tests/test3.ts", Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test4() {
        let err = compile_ts_code("tests/test4.ts", Default::default())
            .await
            .unwrap_err()
            .to_string();
        let err = console::strip_ansi_codes(&err);
        assert!(err.starts_with(
            "The module's source code could not be parsed: Expected ';', '}' or <eof> at file:///"
        ));
        assert!(err.contains("/test4.ts:1:6"));
    }

    fn opts_lib1() -> CompileOptions<'static> {
        CompileOptions {
            extra_default_lib: Some("tests/test5-lib1.d.ts"),
            ..Default::default()
        }
    }

    fn opts_lib2() -> CompileOptions<'static> {
        CompileOptions {
            extra_default_lib: Some("tests/test5-lib2.d.ts"),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test5() -> Result<()> {
        compile_ts_code("tests/test5.ts", opts_lib1()).await?;
        compile_ts_code("tests/test5.ts", opts_lib2()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test6() -> Result<()> {
        compile_ts_code("tests/test6.ts", opts_lib1()).await?;
        compile_ts_code("tests/test6.ts", opts_lib2()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test7() -> Result<()> {
        compile_ts_code("tests/test7.ts", Default::default()).await?;
        Ok(())
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
        assert_eq!(err, "No such file or directory (os error 2)");
        Ok(())
    }

    #[tokio::test]
    async fn no_extension() -> Result<()> {
        let err = compile_ts_code("/no/such/file", Default::default())
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(err, "No such file or directory (os error 2)");
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
    async fn header_entries() -> Result<()> {
        let f = write_temp(
            br#"
export function foo(h: Headers) {
    return h.entries;
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
        assert_eq!(
            err,
            "Relative import path \"bar\" not prefixed with / or ./ or ../"
        );
        Ok(())
    }

    #[tokio::test]
    async fn handlebars() -> Result<()> {
        let f = write_temp(b"import handlebars from \"https://cdn.skypack.dev/handlebars\";")?;
        compile_ts_code(f.path(), Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn deno_types() -> Result<()> {
        compile_ts_code("tests/deno_types.ts", Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn random_uuid() -> Result<()> {
        let f = write_temp(b"export const foo = crypto.randomUUID();")?;
        compile_ts_code(f.path(), Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn synthetic_default() -> Result<()> {
        compile_ts_code("tests/synthetic_default.ts", Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn missing_deno_types() -> Result<()> {
        let err = compile_ts_code("tests/missing_deno_types.ts", Default::default())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("Could not resolve './deno_types_imp.js' in 'file:///"));
        Ok(())
    }
}
