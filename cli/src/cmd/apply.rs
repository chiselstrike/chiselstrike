// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

pub mod deno;
pub mod node;

use crate::project::{read_manifest, read_to_string, AutoIndex, Module, Optimize};
use crate::proto::chisel_rpc_client::ChiselRpcClient;
use crate::proto::{ChiselApplyRequest, IndexCandidate, PolicyUpdateRequest};
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;

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

/// A map of source file paths to the source code.
///
/// The apply phase performs bunch of processing on the source files. This
/// map contains the final processed source files with the full path name
/// to be shipped to the server. For example, endpoints have a `endpoints/`
/// prefix in the path for the server in cases the server needs to do
/// something special depending on the source file type.
pub(crate) type SourceMap = HashMap<String, String>;

pub(crate) async fn apply(
    server_url: String,
    version: String,
    allow_type_deletion: AllowTypeDeletion,
    type_check: TypeChecking,
) -> Result<()> {
    let manifest = read_manifest().context("Could not read manifest file")?;
    let models = manifest.models()?;
    let endpoints = manifest.endpoints()?;
    let events = manifest.events()?;
    let policies = manifest.policies()?;

    let types_req = crate::ts::parse_types(&models)?;
    let mut policy_req = vec![];

    let entities: Vec<String> = types_req
        .iter()
        .map(|type_req| type_req.name.clone())
        .collect();
    let chiselc_available = is_chiselc_available();
    if !chiselc_available {
        println!(
            "Warning: no ChiselStrike compiler (`chiselc`) found. Some your queries might be slow."
        );
    }
    let optimize = chiselc_available && manifest.optimize == Optimize::Yes;
    let auto_index = chiselc_available && manifest.auto_index == AutoIndex::Yes;
    let (sources, index_candidates) = if manifest.modules == Module::Node {
        node::apply(
            &endpoints,
            &events,
            &entities,
            optimize,
            auto_index,
            &type_check,
        )
        .await
    } else {
        deno::apply(&endpoints, &events, &entities, optimize, auto_index).await
    }?;

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

    let mut client = ChiselRpcClient::connect(server_url.clone()).await?;
    let mut req = ChiselApplyRequest {
        types: types_req,
        sources: Default::default(),
        index_candidates,
        policies: policy_req,
        allow_type_deletion: allow_type_deletion.into(),
        version,
        version_tag,
        app_name,
    };

    // According to the spec
    // (https://html.spec.whatwg.org/multipage/webappapis.html#module-map),
    // "Module maps are used to ensure that imported module scripts
    // are only fetched, parsed, and evaluated once per Document or
    // worker."
    //
    // Since we want to change the modules, we need the server to have
    // a Worker that has never imported them. Do this by first
    // clearing the sources from the server and then restarting it.
    //
    // FIXME: We should have a more fine gained way to recreate just
    // the worker without loading the sources from the DB.
    execute!(client.apply(tonic::Request::new(req.clone())).await);
    req.sources = sources;
    crate::restart(server_url).await?;

    let msg = execute!(client.apply(tonic::Request::new(req)).await);

    println!("Code was applied to the ChiselStrike server. It contained:");
    println!("  - models: {}", msg.types.len());
    println!("  - endpoints: {}", msg.endpoints.len());
    println!("  - event handlers: {}", msg.event_handlers.len());
    println!("  - labels: {}", msg.labels.len());

    Ok(())
}

fn parse_indexes(code: String, entities: &[String]) -> Result<Vec<IndexCandidate>> {
    let mut index_candidates = vec![];
    let indexes = chiselc_output(code, "filter-properties", entities)?;
    let indexes: Value = serde_json::from_str(&indexes)?;
    if let Some(indexes) = indexes.as_array() {
        for index in indexes {
            let entity_name = index["entity_name"].as_str().unwrap().to_string();
            let properties = match index["properties"].as_array() {
                Some(properties) => properties
                    .iter()
                    .map(|prop| prop.as_str().unwrap().to_string())
                    .collect(),
                None => vec![],
            };
            index_candidates.push(IndexCandidate {
                entity_name,
                properties,
            });
        }
    }
    Ok(index_candidates)
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
fn chiselc_spawn(input: &str, output: &str, entities: &[String]) -> Result<tokio::process::Child> {
    let mut args: Vec<&str> = vec![input, "--output", output, "--target", "js"];
    if !entities.is_empty() {
        args.push("-e");
        for entity in entities.iter() {
            args.push(entity);
        }
    }
    let cmd = tokio::process::Command::new(chiselc_cmd()?)
        .args(args)
        .spawn()?;
    Ok(cmd)
}

/// Spawn `chiselc`, wait for the process to complete, and return its output.
fn chiselc_output(code: String, target: &str, entities: &[String]) -> Result<String> {
    let mut args: Vec<&str> = vec!["--target", target];
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
    Ok(output_to_string(&output).unwrap())
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
