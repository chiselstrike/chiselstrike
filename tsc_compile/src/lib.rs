// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{anyhow, Context, Result};
pub use deno_core;
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
use deno_graph::MediaType;
use deno_graph::ModuleGraph;
use deno_graph::ModuleKind;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::future::Future;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug)]
struct DownloadMap {
    // Map a location (url or input file) to what it was compiled to.
    written: HashMap<String, String>,

    // Precomputed module graph
    graph: ModuleGraph,

    diagnostics: String,
}

impl DownloadMap {
    fn new(graph: ModuleGraph) -> DownloadMap {
        DownloadMap {
            graph,
            written: Default::default(),
            diagnostics: Default::default(),
        }
    }
}

fn fetch_impl(map: &mut DownloadMap, path: String, base: String) -> Result<String> {
    let url = Url::parse(&base).unwrap();
    let resolved = map
        .graph
        .resolve_dependency(&path, &url, true)
        .ok_or_else(|| anyhow!("Could not resolve '{}' in '{}'", path, url))?
        .clone();
    Ok(resolved.to_string())
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

fn read_impl(map: &mut DownloadMap, path: String) -> Result<String> {
    let url = Url::parse(&path)?;
    let module = match map.graph.try_get(&url) {
        Ok(Some(m)) => m,
        Ok(None) => anyhow::bail!("URL was not loaded"),
        Err(e) => return Err(e.into()),
    };
    Ok((**module.maybe_source.as_ref().unwrap()).clone())
}

#[op]
fn read(s: &mut OpState, path: String) -> Result<String> {
    if let Some(map) = s.try_borrow_mut::<DownloadMap>() {
        read_impl(map, path)
    } else {
        let content = tsc_compile_build::read(&path);
        if content.is_empty() {
            panic!("Unexpected file during bootstrap: {}", path);
        }
        Ok(content.to_string())
    }
}

fn write_impl(map: &mut DownloadMap, path: String, content: String) -> Result<()> {
    map.written.insert(path, content);
    Ok(())
}

#[op]
fn write(s: &mut OpState, path: String, content: String) -> Result<()> {
    if let Some(map) = s.try_borrow_mut::<DownloadMap>() {
        write_impl(map, path, content)
    } else {
        // Don't write anything during bootstrap.
        Ok(())
    }
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

// This is similar to PathBuf::push, but folds "/./" and "/../". This
// is similar to what TSC own path handling does.
fn join_path(mut base: PathBuf, rel: &Path) -> PathBuf {
    for c in rel.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => return rel.to_path_buf(),
            Component::CurDir => {}
            Component::ParentDir => assert!(base.pop()),
            Component::Normal(p) => base.push(p),
        }
    }
    base
}

// Paths are passed to javascript, which uses UTF-16, no point in
// pretending we can handle non unicode PathBufs.
fn abs(path: &str) -> String {
    let p = join_path(env::current_dir().unwrap(), Path::new(path));
    p.into_os_string().into_string().unwrap()
}

fn without_extension(path: &str) -> &str {
    for suffix in [".d.ts", ".ts", ".js"] {
        if let Some(s) = path.strip_suffix(suffix) {
            return s;
        }
    }
    path
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

// If the given source can be made relative to one of the urls, return
// the relative path to, and the original file name of, that url.
fn find_relative<'a>(urls: &HashMap<Url, &'a str>, source: &Url) -> Option<(String, &'a str)> {
    for (url, name) in urls {
        if let Some(rel) = url.make_relative(source) {
            return Some((rel, name));
        }
    }
    None
}

pub struct Compiler {
    pub runtime: JsRuntime,
}

impl Compiler {
    pub fn new(use_snapshot: bool) -> Compiler {
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

        let startup_snapshot = if use_snapshot {
            Some(Snapshot::Static(SNAPSHOT))
        } else {
            None
        };
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![ext],
            startup_snapshot,
            ..Default::default()
        });

        if !use_snapshot {
            for (p, code) in tsc_compile_build::JS_FILES {
                runtime.execute_script(p, code).unwrap();
            }
        }

        Compiler { runtime }
    }

    pub async fn compile_ts_code(
        &mut self,
        file_names: &[&str],
        opts: CompileOptions<'_>,
    ) -> Result<HashMap<String, String>> {
        let mut url_to_name: HashMap<Url, &str> = HashMap::new();
        let mut urls = Vec::new();
        for name in file_names {
            let url = Url::parse(&format!("file://{}", abs(name)))?;
            urls.push((url.clone(), ModuleKind::Esm));
            url_to_name.insert(url, name);
        }

        let mut extra_libs = HashMap::new();
        let mut to_url = HashMap::new();
        for (k, v) in &opts.extra_libs {
            let url = Url::parse(&("chisel:///".to_string() + k + ".ts")).unwrap();
            extra_libs.insert(url.clone(), v.clone());
            to_url.insert(k.clone(), url);
        }

        let mut loader = ModuleLoader { extra_libs };
        let resolver = ModuleResolver { extra_libs: to_url };

        let extra_default_lib = opts
            .extra_default_lib
            .map(|path| format!("file://{}", abs(path)));

        let maybe_imports = if let Some(path) = &extra_default_lib {
            let dummy_url = Url::parse("chisel://std").unwrap();
            Some(vec![(dummy_url, vec![path.to_string()])])
        } else {
            None
        };

        let graph = deno_graph::create_graph(
            urls.clone(),
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

        self.runtime
            .op_state()
            .borrow_mut()
            .put(DownloadMap::new(graph));

        let global_context = self.runtime.global_context();
        {
            let scope = &mut self.runtime.handle_scope();
            let lib = match extra_default_lib {
                Some(v) => v8::String::new(scope, &v).unwrap().into(),
                None => v8::undefined(scope).into(),
            };
            let global_proxy = global_context.open(scope).global(scope);
            let compile: v8::Local<v8::Function> =
                get_member(global_proxy, scope, "compile").unwrap();
            let emit_declarations = v8::Boolean::new(scope, opts.emit_declarations).into();

            for url in &urls {
                let file = v8::String::new(scope, url.0.as_str()).unwrap().into();
                compile
                    .call(scope, global_proxy.into(), &[file, lib, emit_declarations])
                    .unwrap();
            }
        }

        let op_state = self.runtime.op_state();
        let mut op_state = op_state.borrow_mut();
        let mut map = op_state.take::<DownloadMap>();
        if map.diagnostics.is_empty() {
            let mut prefix_map: HashMap<&str, &Url> = HashMap::default();
            let mut ret: HashMap<String, String> = HashMap::default();
            for m in map.graph.modules() {
                let url = &m.specifier;
                prefix_map.insert(without_extension(url.as_str()), url);
                if m.media_type == MediaType::JavaScript {
                    let source = m.maybe_source.as_ref().unwrap().to_string();
                    map.written.insert(url.to_string(), source);
                }
            }
            for (k, v) in map.written {
                let prefix = without_extension(&k);
                let is_dts = k.ends_with(".d.ts");
                let source = prefix_map[prefix];
                let source = if let Some(n) = url_to_name.get(source) {
                    n.to_string()
                } else if is_dts || source.scheme() == "chisel" {
                    continue;
                } else if let Some((rel, source)) = find_relative(&url_to_name, source) {
                    let dir_path = Path::new(source).parent().unwrap().to_path_buf();
                    join_path(dir_path.clone(), Path::new(&rel))
                        .display()
                        .to_string()
                } else {
                    source.to_string()
                };
                let key = if is_dts {
                    without_extension(&source).to_string() + ".d.ts"
                } else {
                    source
                };
                ret.insert(key, v);
            }
            Ok(ret)
        } else {
            Err(anyhow!("Compilation failed:\n{}", map.diagnostics))
        }
    }
}

pub async fn compile_ts_code(
    file_names: &[&str],
    opts: CompileOptions<'_>,
) -> Result<HashMap<String, String>> {
    let mut compiler = Compiler::new(true);
    compiler.compile_ts_code(file_names, opts).await
}

#[cfg(test)]
mod tests {
    use super::abs;
    use super::compile_ts_code;
    use super::CompileOptions;
    use anyhow::Result;
    use deno_core::anyhow;
    use std::future::Future;
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
        let written = compile_ts_code(&[path], opts).await.unwrap();
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
        let written = compile_ts_code(&[path], Default::default()).await.unwrap();
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
        compile_ts_code(&["tests/test3.ts"], Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test4() {
        let err = compile_ts_code(&["tests/test4.ts"], Default::default())
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
        compile_ts_code(&["tests/test5.ts"], opts_lib1()).await?;
        compile_ts_code(&["tests/test5.ts"], opts_lib2()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test6() -> Result<()> {
        compile_ts_code(&["tests/test6.ts"], opts_lib1()).await?;
        compile_ts_code(&["tests/test6.ts"], opts_lib2()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test7() -> Result<()> {
        compile_ts_code(&["tests/test7.ts"], Default::default()).await?;
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
        compile_ts_code(&[f.path()], opts).await?;
        Ok(())
    }

    #[tokio::test]
    async fn diagnostics() -> Result<()> {
        for _ in 0..2 {
            let f = write_temp(b"export {}; zed;")?;
            let err = compile_ts_code(&[f.path()], Default::default()).await;
            let err = err.unwrap_err().to_string();
            assert!(err.contains("Cannot find name 'zed'"));
        }
        Ok(())
    }

    #[tokio::test]
    async fn property_constructor_not_strict() -> Result<()> {
        let f = write_temp(b"export class Foo { a: number };")?;
        compile_ts_code(&[f.path()], Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn missing_file() -> Result<()> {
        let err = compile_ts_code(&["/no/such/file.ts"], Default::default())
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(err, "No such file or directory (os error 2)");
        Ok(())
    }

    #[tokio::test]
    async fn no_extension() -> Result<()> {
        let err = compile_ts_code(&["/no/such/file"], Default::default())
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
        compile_ts_code(&[f.path()], Default::default()).await?;
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
        compile_ts_code(&[f.path()], Default::default()).await?;
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
        compile_ts_code(&[f.path()], Default::default()).await?;
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
        let err = compile_ts_code(&[path], Default::default())
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
        compile_ts_code(&[f.path()], Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn deno_types() -> Result<()> {
        compile_ts_code(&["tests/deno_types.ts"], Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn random_uuid() -> Result<()> {
        let f = write_temp(b"export const foo = crypto.randomUUID();")?;
        compile_ts_code(&[f.path()], Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn synthetic_default() -> Result<()> {
        compile_ts_code(&["tests/synthetic_default.ts"], Default::default()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn missing_deno_types() -> Result<()> {
        let err = compile_ts_code(&["tests/missing_deno_types.ts"], Default::default())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("Could not resolve './deno_types_imp.js' in 'file:///"));
        Ok(())
    }

    #[tokio::test]
    async fn wrong_type() {
        // Test where tsc produced errors point to.
        let err = compile_ts_code(&["tests/wrong_type.ts"], Default::default())
            .await
            .unwrap_err()
            .to_string();
        let err = console::strip_ansi_codes(&err);
        assert!(err.contains(
            "tests/wrong_type.ts:1:14 - error TS2322: Type 'number' is not assignable to type 'string'."
        ));
    }

    #[tokio::test]
    async fn wrong_type_in_import() {
        // Test where tsc produced errors in imports point to.
        let err = compile_ts_code(&["tests/wrong_type_import.ts"], Default::default())
            .await
            .unwrap_err()
            .to_string();
        let err = console::strip_ansi_codes(&err);
        assert!(err.contains(
            "tests/wrong_type.ts:1:14 - error TS2322: Type 'number' is not assignable to type 'string'"
        ));
    }

    async fn test_with_path_variants<F, Fut>(func: F, path: &str)
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = ()>,
    {
        func(path.to_string()).await;
        func(format!("./{}", path)).await;
        func(abs(path)).await;
    }

    async fn check_import(path: String, suffix_a: &str, suffix_b: &str) {
        let import = format!("{}{}", path.strip_suffix(suffix_a).unwrap(), suffix_b);
        let written = compile_ts_code(&[&path], Default::default()).await.unwrap();
        let mut keys: Vec<_> = written.keys().collect();
        keys.sort_unstable();
        let mut expected = vec![import.as_str(), path.as_str()];
        expected.sort_unstable();
        assert_eq!(keys, expected);
    }

    async fn check_output_imported(path: String) {
        check_import(path, "a.ts", "b.ts").await;
    }

    #[tokio::test]
    async fn output_imported() {
        test_with_path_variants(check_output_imported, "tests/output_imported_a.ts").await;
    }

    async fn check_import_js(path: String) {
        check_import(path, "a.ts", "b.js").await;
    }

    #[tokio::test]
    async fn import_js() {
        test_with_path_variants(check_import_js, "tests/import_js_a.ts").await;
    }

    async fn check_pure_js(path: String) {
        check_import(path, "a.js", "b.js").await;
    }

    #[tokio::test]
    async fn pure_js() {
        test_with_path_variants(check_pure_js, "tests/import_js_a.js").await;
    }

    async fn check_relative(path: String) {
        check_import(path, "_a/foo.ts", "_b/bar.ts").await;
    }

    #[tokio::test]
    async fn relative() {
        test_with_path_variants(check_relative, "tests/relative_a/foo.ts").await;
    }
}
