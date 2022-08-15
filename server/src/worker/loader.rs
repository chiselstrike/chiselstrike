use anyhow::{anyhow, bail, Result};
use deno_core::url::Url;
use futures::FutureExt;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

/// The loader is used by Deno when V8 resolves and loads modules.
#[derive(Debug)]
pub struct ModuleLoader {
    modules: Arc<HashMap<String, String>>,
}

impl ModuleLoader {
    pub fn new(modules: Arc<HashMap<String, String>>) -> ModuleLoader {
        ModuleLoader { modules }
    }
}

impl deno_core::ModuleLoader for ModuleLoader {
    fn resolve(&self, specifier: &str, referrer: &str, _is_main: bool) -> Result<Url> {
        if specifier == "@chiselstrike/api" {
            return Ok(Url::parse("chisel:///api.ts")?);
        }
        Ok(deno_core::resolve_import(specifier, referrer)?)
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
    let path = url.path().trim_start_matches('/').trim_end_matches(".ts");
    match api::SOURCES_JS.get(path) {
        Some(code) => Ok(source_from_code(&url, code)),
        None => bail!("Undefined internal chisel module {}", url),
    }
}

fn source_from_code(url: &Url, code: &str) -> deno_core::ModuleSource {
    deno_core::ModuleSource {
        code: code.as_bytes().into(),
        module_type: deno_core::ModuleType::JavaScript,
        module_url_specified: url.to_string(),
        module_url_found: url.to_string(),
    }
}
