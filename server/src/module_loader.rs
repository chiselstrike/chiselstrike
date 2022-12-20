use anyhow::{anyhow, bail, Result};
use deno_core::url::Url;
use futures::FutureExt;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

/// The loader is used by Deno when V8 resolves and loads modules.
#[derive(Debug)]
pub struct ModuleLoader {
    /// Maps fully qualified module specifiers (absolute URLs) to transpiled JavaScript sources.
    modules: Arc<HashMap<String, String>>,
}

impl ModuleLoader {
    pub fn new(modules: Arc<HashMap<String, String>>) -> ModuleLoader {
        ModuleLoader { modules }
    }
}

impl deno_core::ModuleLoader for ModuleLoader {
    fn resolve(&self, specifier: &str, referrer: &str, _is_main: bool) -> Result<Url> {
        Ok(if specifier == "@chiselstrike/api" {
            Url::parse("chisel://api/api.ts").unwrap()
        } else if let Some(path) = NODE_POLYFILLS.get(specifier) {
            Url::parse(&format!("chisel://deno-std/{}", path)).unwrap()
        } else {
            deno_core::resolve_import(specifier, referrer)?
        })
    }

    fn load(
        &self,
        module_specifier: &Url,
        maybe_referrer: Option<Url>,
        is_dyn_import: bool,
    ) -> Pin<Box<deno_core::ModuleSourceFuture>> {
        if module_specifier.scheme() == "chisel" {
            let url = module_specifier.clone();
            return async move { load_chisel_module(url) }.boxed_local();
        }

        if let Some(code) = self.modules.get(module_specifier.as_str()) {
            let source = source_from_code(module_specifier, code);
            async move { Ok(source) }.boxed_local()
        } else {
            let err = anyhow!(
                "chiseld cannot load module {} at runtime{}{}",
                module_specifier,
                maybe_referrer
                    .map(|url| format!(" (referrer: {})", url))
                    .unwrap_or_default(),
                if is_dyn_import {
                    " (dynamic import)"
                } else {
                    ""
                },
            );
            async move { Err(err) }.boxed_local()
        }
    }
}

fn load_chisel_module(url: Url) -> Result<deno_core::ModuleSource> {
    // note that the module URL may end with ".ts", but we must return the transpiled JavaScript
    match url.domain() {
        Some("api") => {
            let path = url.path().trim_start_matches('/').trim_end_matches(".ts");
            if let Some(code) = api::SOURCES_JS.get(path) {
                return Ok(source_from_code(&url, code));
            }
        }
        Some("deno-std") => {
            let path = url.path().trim_start_matches('/');
            if let Some(code) = deno_std::SOURCES_JS.get(path) {
                return Ok(source_from_code(&url, code));
            }
        }
        _ => {}
    }
    bail!("Undefined internal chisel module {}", url)
}

fn source_from_code(url: &Url, code: &str) -> deno_core::ModuleSource {
    deno_core::ModuleSource {
        code: code.as_bytes().into(),
        module_type: deno_core::ModuleType::JavaScript,
        module_url_specified: url.to_string(),
        module_url_found: url.to_string(),
    }
}

lazy_static! {
    static ref NODE_POLYFILLS: HashMap<&'static str, &'static str> = vec![
        // this list is taken from deno/cli/node/mod.rs
        ("assert", "node/assert.ts"),
        ("assert/strict", "node/assert/strict.ts"),
        ("async_hooks", "node/async_hooks.ts"),
        ("buffer", "node/buffer.ts"),
        ("child_process", "node/child_process.ts"),
        ("cluster", "node/cluster.ts"),
        ("console", "node/console.ts"),
        ("constants", "node/constants.ts"),
        ("crypto", "node/crypto.ts"),
        ("dgram", "node/dgram.ts"),
        ("dns", "node/dns.ts"),
        ("dns/promises", "node/dns/promises.ts"),
        ("domain", "node/domain.ts"),
        ("events", "node/events.ts"),
        ("fs", "node/fs.ts"),
        ("fs/promises", "node/fs/promises.ts"),
        ("http", "node/http.ts"),
        ("https", "node/https.ts"),
        ("net", "node/net.ts"),
        ("os", "node/os.ts"),
        ("path", "node/path.ts"),
        ("path/posix", "node/path/posix.ts"),
        ("path/win32", "node/path/win32.ts"),
        ("perf_hooks", "node/perf_hooks.ts"),
        ("process", "node/process.ts"),
        ("querystring", "node/querystring.ts"),
        ("readline", "node/readline.ts"),
        ("stream", "node/stream.ts"),
        ("stream/consumers", "node/stream/consumers.mjs"),
        ("stream/promises", "node/stream/promises.mjs"),
        ("stream/web", "node/stream/web.ts"),
        ("string_decoder", "node/string_decoder.ts"),
        ("sys", "node/sys.ts"),
        ("timers", "node/timers.ts"),
        ("timers/promises", "node/timers/promises.ts"),
        ("tls", "node/tls.ts"),
        ("tty", "node/tty.ts"),
        ("url", "node/url.ts"),
        ("util", "node/util.ts"),
        ("util/types", "node/util/types.ts"),
        ("v8", "node/v8.ts"),
        ("vm", "node/vm.ts"),
        ("worker_threads", "node/worker_threads.ts"),
        ("zlib", "node/zlib.ts"),
    ]
    .into_iter()
    .collect();
}
