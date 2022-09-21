// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

pub mod deno;
pub mod node;

use crate::project::{read_manifest, read_to_string, AutoIndex, Module, Optimize};
use crate::proto::chisel_rpc_client::ChiselRpcClient;
use crate::proto::{ApplyRequest, IndexCandidate, PolicyUpdateRequest};
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::env;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
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

pub(crate) async fn apply(
    server_url: String,
    version_id: String,
    allow_type_deletion: AllowTypeDeletion,
    type_check: TypeChecking,
) -> Result<()> {
    let cwd = env::current_dir()?;
    let manifest = read_manifest(&cwd).context("Could not read manifest file")?;
    let models = manifest.models(&cwd)?;
    let route_map = manifest.route_map(&cwd)?;
    let topic_map = manifest.topic_map(&cwd)?;
    let policies = manifest.policies(&cwd)?;

    let types_req = crate::ts::parse_types(&models)?;
    let mut policy_req = vec![];

    let entities: Vec<String> = types_req
        .iter()
        .map(|type_req| type_req.name.clone())
        .collect();
    let chiselc_available = is_chiselc_available();
    if !chiselc_available {
        println!(
            "Warning: no ChiselStrike compiler (`chiselc`) found. Some of your queries might be slow."
        );
    }
    let optimize = chiselc_available && manifest.optimize == Optimize::Yes;
    let auto_index = chiselc_available && manifest.auto_index == AutoIndex::Yes;
    let (modules, index_candidates) = match manifest.modules {
        Module::Node => {
            node::apply(
                route_map,
                topic_map,
                &entities,
                optimize,
                auto_index,
                &type_check,
            )
            .await?
        }
        Module::Deno => deno::apply(route_map, topic_map, &entities, optimize, auto_index).await?,
    };

    for p in &policies {
        policy_req.push(PolicyUpdateRequest {
            policy_config: read_to_string(&p)?,
            path: p.display().to_string(),
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
    let req = ApplyRequest {
        types: types_req,
        modules,
        index_candidates,
        policies: policy_req,
        allow_type_deletion: allow_type_deletion.into(),
        version_id,
        version_tag,
        app_name,
    };

    let msg = execute!(client.apply(tonic::Request::new(req)).await);

    println!("Applied:");
    if !msg.types.is_empty() {
        println!("  {} models", msg.types.len());
    }
    if !msg.event_handlers.is_empty() {
        println!("  {} event handlers", msg.event_handlers.len());
    }
    if !msg.labels.is_empty() {
        println!("  {} labels", msg.labels.len());
    }

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
fn chiselc_spawn(
    input: &Path,
    output: &Path,
    entities: &[String],
) -> Result<tokio::process::Child> {
    let mut args: Vec<&OsStr> = vec![
        input.as_ref(),
        "--output".as_ref(),
        output.as_ref(),
        "--target".as_ref(),
        "js".as_ref(),
    ];
    if !entities.is_empty() {
        args.push("-e".as_ref());
        for entity in entities.iter() {
            args.push(entity.as_ref());
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
