use anyhow::Result;
use deno_runtime::deno_core::futures::FutureExt;
use deno_runtime::deno_core::url::Url;
use deno_runtime::deno_core::{ModuleSource, ModuleSourceFuture, ModuleType};
use deno_runtime::permissions::Permissions;
use deno_runtime::worker::{MainWorker, WorkerOptions};
use deno_runtime::BootstrapOptions;
use std::env;
use std::path::Path;
use std::pin::Pin;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;

/// The loader is used by Deno when V8 resolves and loads modules.
#[derive(Debug)]
struct ModuleLoader;

impl deno_runtime::deno_core::ModuleLoader for ModuleLoader {
    fn resolve(&self, specifier: &str, _referrer: &str, _is_main: bool) -> Result<Url> {
        if specifier == "chisel://generate_reflection.js" {
            Ok(Url::from_str(specifier)?)
        } else {
            panic!("Only generate_reflection.js specifier supported (everything should be included in that bundle)");
        }
    }

    fn load(
        &self,
        module_specifier: &Url,
        _maybe_referrer: Option<Url>,
        _is_dyn_import: bool,
    ) -> Pin<Box<ModuleSourceFuture>> {
        let source = source_from_code(
            module_specifier,
            include_str!(concat!(env!("OUT_DIR"), "/generate_reflection.js")),
        );
        async move { Ok(source) }.boxed_local()
    }
}

fn source_from_code(url: &Url, code: &str) -> ModuleSource {
    ModuleSource {
        code: code.as_bytes().into(),
        module_type: ModuleType::JavaScript,
        module_url_specified: url.to_string(),
        module_url_found: url.to_string(),
    }
}

pub async fn transform_in_place(project_root: &Path) -> Result<()> {
    let module_loader = Rc::new(ModuleLoader);
    let create_web_worker_cb = Arc::new(|_| panic!("Web workers are not supported"));
    let web_worker_preload_module_cb = Arc::new(|_| panic!("Web workers are not supported"));
    let web_worker_pre_execute_module_cb = Arc::new(|_| panic!("Web workers are not supported"));

    let options = WorkerOptions {
        bootstrap: BootstrapOptions {
            args: vec![project_root.to_string_lossy().to_string()],
            ..Default::default()
        },
        web_worker_preload_module_cb,
        web_worker_pre_execute_module_cb,
        create_web_worker_cb,
        module_loader,
        ..Default::default()
    };

    let main_url = Url::parse("chisel://generate_reflection.js")?;
    let permissions = Permissions {
        read: Permissions::new_read(&Some(vec![]), false)?,
        write: Permissions::new_write(&Some(vec![]), false)?,
        ..Permissions::default()
    };

    let mut worker = MainWorker::bootstrap_from_options(main_url.clone(), permissions, options);
    worker.execute_main_module(&main_url).await?;
    worker.run_event_loop(false).await?;
    Ok(())
}
