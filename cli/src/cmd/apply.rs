// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::chisel::chisel_rpc_client::ChiselRpcClient;
use crate::chisel::{ChiselApplyRequest, EndPointCreationRequest, PolicyUpdateRequest};
use crate::project::{read_manifest, read_to_string, Module};
use anyhow::{anyhow, Context, Result};
use compile::compile_ts_code as swc_compile;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tempfile::Builder;
use tempfile::NamedTempFile;
use tokio::task::{spawn_blocking, JoinHandle};
use tsc_compile::compile_ts_code;
use tsc_compile::CompileOptions;

pub(crate) enum AllowTypeDeletion {
    No,
    Yes,
}

impl From<AllowTypeDeletion> for bool {
    fn from(v: AllowTypeDeletion) -> Self {
        match v {
            AllowTypeDeletion::No => false,
            AllowTypeDeletion::Yes => true,
        }
    }
}

impl From<bool> for AllowTypeDeletion {
    fn from(v: bool) -> Self {
        match v {
            false => AllowTypeDeletion::No,
            true => AllowTypeDeletion::Yes,
        }
    }
}

#[derive(Copy, Clone)]
pub(crate) enum TypeChecking {
    No,
    Yes,
}

impl From<TypeChecking> for bool {
    fn from(v: TypeChecking) -> Self {
        match v {
            TypeChecking::No => false,
            TypeChecking::Yes => true,
        }
    }
}

impl From<bool> for TypeChecking {
    fn from(v: bool) -> Self {
        match v {
            false => TypeChecking::No,
            true => TypeChecking::Yes,
        }
    }
}

pub(crate) async fn apply<S: ToString>(
    server_url: String,
    version: S,
    allow_type_deletion: AllowTypeDeletion,
    type_check: TypeChecking,
    include_paths: HashSet<PathBuf>,
) -> Result<()> {
    let version = version.to_string();

    let manifest = read_manifest().with_context(|| "Reading manifest file".to_string())?;
    let mut models = manifest.models()?;
    let mut endpoints = manifest.endpoints()?;
    let mut policies = manifest.policies()?;

    if !include_paths.is_empty() {
        models = models
            .into_iter()
            .filter(|path| include_paths.contains(&fs::canonicalize(path).unwrap()))
            .collect();
        endpoints = endpoints
            .into_iter()
            .filter(|endpoint| {
                include_paths.contains(&fs::canonicalize(&endpoint.file_path).unwrap())
            })
            .collect();
        policies = policies
            .into_iter()
            .filter(|path| include_paths.contains(&fs::canonicalize(path).unwrap()))
            .collect();
    }
    let types_req = crate::ts::parse_types(&models)?;
    let mut endpoints_req = vec![];
    let mut policy_req = vec![];

    let import_str = "import * as ChiselAlias from \"@chiselstrike/api\";
         declare global {
             var Chisel: typeof ChiselAlias;
         }"
    .to_string();
    let import_temp = to_tempfile(&import_str, ".d.ts")?;

    let mut types_string = String::new();
    for t in &models {
        types_string += &read_to_string(&t)?;
    }

    if manifest.modules == Module::Node {
        let tsc = match type_check {
            TypeChecking::Yes => Some(npx(
                "tsc",
                &["--noemit", "--pretty", "--allowJs", "--checkJs"],
            )),
            TypeChecking::No => None,
        };

        let mut endpoint_futures = vec![];
        let mut keep_tmp_alive = vec![];

        let cwd = env::current_dir()?;

        for endpoint in endpoints.iter() {
            let out = Builder::new().suffix(".ts").tempfile()?;
            let bundler_output_file = out.path().to_str().unwrap().to_owned();

            let mut f = Builder::new().suffix(".ts").tempfile()?;
            let inner = f.as_file_mut();
            let mut import_path = endpoint.file_path.to_owned();
            import_path.set_extension("");

            let code = format!(
                "import fun from \"{}/{}\";\nexport default fun",
                cwd.display(),
                import_path.display()
            );
            inner.write_all(code.as_bytes())?;
            inner.flush()?;
            let bundler_entry_fname = f.path().to_str().unwrap().to_owned();
            keep_tmp_alive.push(f);
            keep_tmp_alive.push(out);

            endpoint_futures.push((
                bundler_output_file.clone(),
                npx(
                    "esbuild",
                    &[
                        &bundler_entry_fname,
                        "--bundle",
                        "--color=true",
                        "--target=esnext",
                        "--external:@chiselstrike",
                        "--format=esm",
                        "--tree-shaking=true",
                        "--tsconfig=./tsconfig.json",
                        "--platform=node",
                        &format!("--outfile={}", bundler_output_file),
                    ],
                ),
            ));
        }

        for (endpoint, execution) in endpoints.iter().zip(endpoint_futures.into_iter()) {
            let (bundler_output_file, res) = execution;
            let res = res.await.unwrap()?;

            if !res.status.success() {
                let out = String::from_utf8(res.stdout).expect("command output not utf-8");
                let err = String::from_utf8(res.stderr).expect("command output not utf-8");

                return Err(anyhow!(
                    "compiling endpoint {}",
                    endpoint.file_path.display()
                ))
                .with_context(|| format!("{}\n{}", out, err));
            }
            let code = read_to_string(bundler_output_file)?;

            endpoints_req.push(EndPointCreationRequest {
                path: endpoint.name.clone(),
                code,
            });
        }

        if let Some(tsc) = tsc {
            let tsc_res = tsc.await.unwrap()?;
            if !tsc_res.status.success() {
                let out = String::from_utf8(tsc_res.stdout).expect("command output not utf-8");
                let err = String::from_utf8(tsc_res.stderr).expect("command output not utf-8");
                anyhow::bail!("{}\n{}", out, err);
            }
        }
    } else {
        let mods: HashMap<String, String> = [(
            "@chiselstrike/api".to_string(),
            api::chisel_d_ts().to_string(),
        )]
        .into_iter()
        .collect();

        for f in endpoints.iter() {
            let ext = f.file_path.extension().unwrap().to_str().unwrap();
            let path = f.file_path.to_str().unwrap();

            let code = if ext == "ts" {
                let opts = CompileOptions {
                    extra_default_lib: Some(import_temp.path().to_str().unwrap()),
                    extra_libs: mods.clone(),
                    ..Default::default()
                };
                let mut code = compile_ts_code(path, opts)
                    .await
                    .with_context(|| format!("parsing endpoint /{}/{}", version, f.name))?;
                code.remove(path).unwrap()
            } else {
                read_to_string(&f.file_path)?
            };

            let code = types_string.clone() + &code;
            let code = swc_compile(code)?;
            endpoints_req.push(EndPointCreationRequest {
                path: f.name.clone(),
                code,
            });
        }
    }

    for p in policies {
        policy_req.push(PolicyUpdateRequest {
            policy_config: read_to_string(p)?,
        });
    }

    let mut client = ChiselRpcClient::connect(server_url).await?;
    let msg = execute!(
        client
            .apply(tonic::Request::new(ChiselApplyRequest {
                types: types_req,
                endpoints: endpoints_req,
                policies: policy_req,
                allow_type_deletion: allow_type_deletion.into(),
                version,
            }))
            .await
    );

    for ty in msg.types {
        println!("Model defined: {}", ty);
    }

    for end in msg.endpoints {
        println!("End point defined: {}", end);
    }

    for lbl in msg.labels {
        println!("Policy defined for label {}", lbl);
    }

    Ok(())
}

fn to_tempfile(data: &str, suffix: &str) -> Result<NamedTempFile> {
    let mut f = Builder::new().suffix(suffix).tempfile()?;
    let inner = f.as_file_mut();
    inner.write_all(data.as_bytes())?;
    inner.flush()?;
    Ok(f)
}

fn npx(command: &str, args: &[&str]) -> JoinHandle<Result<std::process::Output>> {
    let mut cmd = std::process::Command::new("npx");
    cmd.arg(command).args(args);

    spawn_blocking(move || {
        cmd.output()
            .with_context(|| "trying to execute `npx esbuild`. Is npx on your PATH?".to_string())
    })
}
