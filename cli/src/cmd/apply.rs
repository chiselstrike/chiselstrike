// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::chisel::chisel_rpc_client::ChiselRpcClient;
use crate::chisel::{ChiselApplyRequest, EndPointCreationRequest, PolicyUpdateRequest};
use crate::project::{read_manifest, read_to_string, Module, Optimize};
use anyhow::{anyhow, Context, Result};
use compile::compile_ts_code as swc_compile;
use std::collections::HashMap;
use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use tempfile::Builder;
use tempfile::NamedTempFile;
use tokio::task::{spawn_blocking, JoinHandle};
use tsc_compile::compile_ts_code;
use tsc_compile::CompileOptions;

static DEFAULT_APP_NAME: &str = "ChiselStrike Application";

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
) -> Result<()> {
    let version = version.to_string();

    let manifest = read_manifest().with_context(|| "Reading manifest file".to_string())?;
    let models = manifest.models()?;
    let endpoints = manifest.endpoints()?;
    let policies = manifest.policies()?;

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
    let entities: Vec<String> = types_req
        .iter()
        .map(|type_req| type_req.name.clone())
        .collect();
    let use_chiselc = is_chiselc_available() && manifest.optimize == Optimize::Yes;
    if manifest.modules == Module::Node {
        let tsc = match type_check {
            TypeChecking::Yes => Some(npx(
                "tsc",
                &["--noemit", "--pretty", "--allowJs", "--checkJs"],
                None,
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

            if use_chiselc {
                // Spawn `chiselc` and pipe its output to `esbuild`.
                let chiselc_cmd = chiselc_spawn(&bundler_entry_fname, &entities)?;
                let cmd = npx(
                    "esbuild",
                    &[
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
                    chiselc_cmd.stdout,
                );
                endpoint_futures.push((bundler_output_file.clone(), cmd));
            } else {
                let cmd = npx(
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
                    None,
                );
                endpoint_futures.push((bundler_output_file.clone(), cmd));
            }
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
            let code = if use_chiselc {
                let output = chiselc_output(code, &entities)?;
                output_to_string(&output).unwrap()
            } else {
                swc_compile(code)?
            };
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

    let package = match read_to_string("./package.json") {
        Ok(x) => {
            let val: serde_json::Result<serde_json::Value> = serde_json::from_str(&x);
            match val {
                Ok(val) => val,
                Err(_) => serde_json::json!("{}"),
            }
        }
        Err(_) => serde_json::json!("{}"),
    };

    let git_version = get_git_version();

    let app_name = package["name"]
        .as_str()
        .unwrap_or(DEFAULT_APP_NAME)
        .to_owned();
    let mut version_tag = package["version"].as_str().unwrap_or("").to_owned();

    version_tag = match git_version {
        Some(v) => {
            if version_tag.is_empty() {
                v
            } else {
                format!("{}-{}", version_tag, v)
            }
        }
        None => version_tag,
    };

    let mut client = ChiselRpcClient::connect(server_url).await?;
    let msg = execute!(
        client
            .apply(tonic::Request::new(ChiselApplyRequest {
                types: types_req,
                endpoints: endpoints_req,
                policies: policy_req,
                allow_type_deletion: allow_type_deletion.into(),
                version,
                version_tag,
                app_name,
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

fn output_to_string(out: &std::process::Output) -> Option<String> {
    Some(
        std::str::from_utf8(&out.stdout)
            .expect("command output not utf-8")
            .trim()
            .to_owned(),
    )
}

fn chiselc_cmd() -> Result<PathBuf> {
    let mut cmd = std::env::current_exe()?;
    cmd.pop();
    cmd.push("chiselc");
    Ok(cmd)
}

fn is_chiselc_available() -> bool {
    let cmd = match chiselc_cmd() {
        Ok(cmd) => cmd,
        _ => return false,
    };
    let mut cmd = std::process::Command::new(cmd);
    cmd.args(&["--version"]);
    match cmd.output() {
        Ok(output) => output.status.success(),
        _ => false,
    }
}

/// Spawn `chiselc` and return a reference to the child process.
fn chiselc_spawn(input: &str, entities: &[String]) -> Result<std::process::Child> {
    let mut args: Vec<&str> = vec![input, "--target", "js"];
    if !entities.is_empty() {
        args.push("-e");
        for entity in entities.iter() {
            args.push(entity);
        }
    }
    let cmd = std::process::Command::new(chiselc_cmd()?)
        .args(args)
        .stdout(Stdio::piped())
        .spawn()?;
    Ok(cmd)
}

/// Spawn `chiselc`, wait for the process to complete, and return its output.
fn chiselc_output(code: String, entities: &[String]) -> Result<std::process::Output> {
    let mut args: Vec<&str> = vec!["--target", "js"];
    if !entities.is_empty() {
        args.push("-e");
        for entity in entities.iter() {
            args.push(entity);
        }
    }
    let mut cmd = std::process::Command::new(chiselc_cmd()?)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut stdin = cmd.stdin.take().expect("Failed to open stdin");
    std::thread::spawn(move || {
        stdin
            .write_all(code.as_bytes())
            .expect("Failed to write to stdin");
    });
    let output = cmd.wait_with_output().expect("Failed to read stdout");
    Ok(output)
}

fn npx(
    command: &str,
    args: &[&str],
    stdin: Option<std::process::ChildStdout>,
) -> JoinHandle<Result<std::process::Output>> {
    let mut cmd = std::process::Command::new("npx");
    cmd.arg(command).args(args);

    if let Some(stdin) = stdin {
        cmd.stdin(stdin);
    }

    spawn_blocking(move || {
        cmd.output()
            .with_context(|| "trying to execute `npx esbuild`. Is npx on your PATH?".to_string())
    })
}

fn get_git_version() -> Option<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["describe", "--exact-match", "--tags"]);

    let tag = cmd.output().ok()?;
    if tag.status.success() {
        return output_to_string(&tag);
    }

    let mut cmd = std::process::Command::new("git");
    cmd.args(["rev-parse", "--short", "HEAD"]);

    let sha = cmd.output().ok()?;
    if sha.status.success() {
        return output_to_string(&sha);
    }
    None
}
