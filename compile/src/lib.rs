// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::{anyhow, Result};
use deno_core::op_sync;
use deno_core::JsRuntime;
use deno_core::OpState;
use deno_core::RuntimeOptions;
use deno_core::Snapshot;
use isahc::ReadResponseExt;
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use url::Url;

#[derive(Debug)]
struct UrlAndContent {
    url: Url,
    content: String,
}

#[derive(Default, Debug)]
struct DownloadMap {
    // map download path to urls and content
    path_to_url_content: HashMap<String, UrlAndContent>,

    // maps url to the download path.
    url_to_path: HashMap<Url, String>,

    // Map a location (url or input file) to what it was compiled to.
    written: HashMap<String, String>,

    // maps absolute path without extension to the input as written.
    input_files: HashMap<String, String>,

    diagnostics: String,
}

impl DownloadMap {
    fn len(&self) -> usize {
        self.path_to_url_content.len()
    }
    fn insert(&mut self, path: String, url: Url, content: String) {
        let url_and_content = UrlAndContent { url, content };
        self.url_to_path
            .insert(url_and_content.url.clone(), path.clone());
        self.path_to_url_content.insert(path, url_and_content);
    }
}

thread_local! {
    static FILES: RefCell<DownloadMap> = RefCell::new(DownloadMap::default());
}

fn fetch_aux(map: &mut DownloadMap, path: String, mut base: String) -> Result<String> {
    if let Some(url_and_content) = map.path_to_url_content.get(&base) {
        base = url_and_content.url.to_string();
    } else {
        assert!(base.as_bytes()[0] == b'/');
        base = "file://".to_string() + &base;
    }
    let resolved = deno_core::resolve_import(&path, &base)?;
    if let Some(path) = map.url_to_path.get(&resolved) {
        return Ok(path.clone());
    }

    let text = if resolved.scheme() == "file" {
        fs::read_to_string(resolved.to_file_path().unwrap())?
    } else {
        isahc::get(resolved.to_string())?.text()?
    };

    let n = map.len();
    let extension = path.rsplit_once('.').unwrap().1;
    let path = format!("/path/to/downloaded/files/{}.{}", n, extension);
    map.insert(path.clone(), resolved, text);
    Ok(path)
}

fn fetch(_op_state: &mut OpState, path: String, base: String) -> Result<String> {
    FILES.with(|m| fetch_aux(&mut m.borrow_mut(), path, base))
}

fn read_aux(map: &mut DownloadMap, path: String) -> Result<String> {
    if let Some(c) = map.path_to_url_content.get(&path) {
        return Ok(c.content.clone());
    }
    if let Some(suffix) = path.strip_prefix("/default/lib/location/") {
        macro_rules! inc {
            ( $($e:expr),* ) => {
                match suffix {
                    $(
                        $e => include_str!(concat!("lib/", $e)),
                    )*
                        _ => "",
                }
            };

        }
        let content = inc!(
            "lib.dom.d.ts",
            "lib.es2015.collection.d.ts",
            "lib.es2015.core.d.ts",
            "lib.es2015.d.ts",
            "lib.es2015.generator.d.ts",
            "lib.es2015.iterable.d.ts",
            "lib.es2015.promise.d.ts",
            "lib.es2015.proxy.d.ts",
            "lib.es2015.reflect.d.ts",
            "lib.es2015.symbol.d.ts",
            "lib.es2015.symbol.wellknown.d.ts",
            "lib.es2016.array.include.d.ts",
            "lib.es2016.d.ts",
            "lib.es2017.d.ts",
            "lib.es2017.intl.d.ts",
            "lib.es2017.object.d.ts",
            "lib.es2017.sharedmemory.d.ts",
            "lib.es2017.string.d.ts",
            "lib.es2017.typedarrays.d.ts",
            "lib.es2018.asyncgenerator.d.ts",
            "lib.es2018.asynciterable.d.ts",
            "lib.es2018.d.ts",
            "lib.es2018.intl.d.ts",
            "lib.es2018.promise.d.ts",
            "lib.es2018.regexp.d.ts",
            "lib.es2019.array.d.ts",
            "lib.es2019.d.ts",
            "lib.es2019.object.d.ts",
            "lib.es2019.string.d.ts",
            "lib.es2019.symbol.d.ts",
            "lib.es2020.bigint.d.ts",
            "lib.es2020.d.ts",
            "lib.es2020.intl.d.ts",
            "lib.es2020.promise.d.ts",
            "lib.es2020.sharedmemory.d.ts",
            "lib.es2020.string.d.ts",
            "lib.es2020.symbol.wellknown.d.ts",
            "lib.es2021.d.ts",
            "lib.es2021.intl.d.ts",
            "lib.es2021.promise.d.ts",
            "lib.es2021.string.d.ts",
            "lib.es2021.weakref.d.ts",
            "lib.es5.d.ts",
            "lib.esnext.d.ts",
            "lib.esnext.intl.d.ts"
        );

        if !content.is_empty() {
            return Ok(content.to_string());
        }
    }
    Ok(fs::read_to_string(path)?)
}

fn read(_op_state: &mut OpState, path: String, _: ()) -> Result<String> {
    FILES.with(|m| read_aux(&mut m.borrow_mut(), path))
}

fn write_aux(map: &mut DownloadMap, mut path: String, content: String) {
    path = path.strip_prefix("chisel:/").unwrap().to_string();
    if let Some(url) = map.path_to_url_content.get(&path) {
        path = url.url.to_string();
    } else {
        let (prefix, is_dts) = match path.strip_suffix(".d.ts") {
            None => (without_extension(&path), false),
            Some(prefix) => (prefix, true),
        };
        path = match map.input_files.get(prefix) {
            Some(path) => path.clone(),
            None => return,
        };
        if is_dts {
            path = without_extension(&path).to_string() + ".d.ts";
        }
    }
    map.written.insert(path, content);
}

fn write(_op_state: &mut OpState, path: String, content: String) -> Result<()> {
    FILES.with(|m| write_aux(&mut m.borrow_mut(), path, content));
    Ok(())
}

fn get_cwd(_op_state: &mut OpState, _: (), _: ()) -> Result<String> {
    let cwd = std::env::current_dir()?;
    Ok(cwd.into_os_string().into_string().unwrap())
}

fn dir_exists(_op_state: &mut OpState, path: String, _: ()) -> Result<bool> {
    return Ok(Path::new(&path).is_dir());
}

fn file_exists(_op_state: &mut OpState, path: String, _: ()) -> Result<bool> {
    return Ok(Path::new(&path).is_file());
}

fn diagnostic(_op_state: &mut OpState, msg: String, _: ()) -> Result<()> {
    FILES.with(|m| {
        let mut borrow = m.borrow_mut();
        assert!(borrow.diagnostics.is_empty());
        borrow.diagnostics = msg;
    });
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
    path.rsplit_once('.').unwrap().0
}

pub static SNAPSHOT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/SNAPSHOT.bin"));

pub fn compile_ts_code(file_name: &str, lib_name: Option<&str>) -> Result<HashMap<String, String>> {
    let lib_name = lib_name.unwrap_or("/default/lib/location/lib.esnext.d.ts");
    FILES.with(|m| {
        let mut borrow = m.borrow_mut();
        borrow.path_to_url_content.clear();
        borrow.url_to_path.clear();
        borrow.written.clear();
        borrow.input_files.clear();
        borrow
            .input_files
            .insert(abs(without_extension(file_name)), file_name.to_string());
    });

    let mut runtime = JsRuntime::new(RuntimeOptions {
        startup_snapshot: Some(Snapshot::Static(SNAPSHOT)),
        ..Default::default()
    });
    runtime.register_op("fetch", op_sync(fetch));
    runtime.register_op("read", op_sync(read));
    runtime.register_op("write", op_sync(write));
    runtime.register_op("get_cwd", op_sync(get_cwd));
    runtime.register_op("dir_exists", op_sync(dir_exists));
    runtime.register_op("file_exists", op_sync(file_exists));
    runtime.register_op("diagnostic", op_sync(diagnostic));
    runtime.sync_ops_cache();

    let global_context = runtime.global_context();
    let scope = &mut runtime.handle_scope();
    let file = v8::String::new(scope, &abs(file_name)).unwrap().into();
    let lib = v8::String::new(scope, &abs(lib_name)).unwrap().into();

    let global_proxy = global_context.open(scope).global(scope);

    let compile: v8::Local<v8::Function> = get_member(global_proxy, scope, "compile").unwrap();

    if !compile
        .call(scope, global_proxy.into(), &[file, lib])
        .unwrap()
        .is_true()
    {
        return Err(FILES.with(|m| anyhow!("Compilation failed:\n{}", m.borrow().diagnostics)));
    }

    FILES.with(|m| {
        let borrow = m.borrow();
        Ok(borrow.written.clone())
    })
}
