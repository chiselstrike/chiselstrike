// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::cmd::apply::chiselc_spawn;
use crate::cmd::apply::parse_indexes;
use crate::cmd::apply::TypeChecking;
use crate::codegen::codegen_root_module;
use crate::events::FileTopicMap;
use crate::project::read_to_string;
use crate::proto::{IndexCandidate, Module};
use crate::routes::FileRouteMap;
use anyhow::{bail, Context, Result};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::{env, fs};
use tsc_reflection;

use super::common::{create_tmp_route_files, create_tmp_topic_files};

pub(crate) async fn apply(
    mut route_map: FileRouteMap,
    mut topic_map: FileTopicMap,
    entities: &[String],
    optimize: bool,
    auto_index: bool,
    type_check: &TypeChecking,
) -> Result<(Vec<Module>, Vec<IndexCandidate>)> {
    let tsc_proc = match type_check {
        TypeChecking::Yes => Some(npx(
            "tsc",
            &["--noemit", "--pretty", "--allowJs", "--checkJs"],
        )?),
        TypeChecking::No => None,
    };

    // ideally we would call this in parallel with the bundle, but npx doesn't like this very much
    // See #1642
    if let Some(tsc_proc) = tsc_proc {
        let tsc_output = tsc_proc
            .wait_with_output()
            .await
            .context("Could not run tsc to type-check the code")?;
        ensure_success(tsc_output).context("Type-checking with tsc failed")?;
    }

    let cwd = env::current_dir()?;

    let route_gen_dir = cwd.join(".routegen");
    let event_gen_dir = cwd.join(".eventgen");

    route_map = create_tmp_route_files(route_map, &route_gen_dir)?;
    topic_map = create_tmp_topic_files(topic_map, &event_gen_dir)?;
    tsc_reflection::transform_in_place(&cwd, &route_gen_dir, false).await?;

    let mut index_candidates = vec![];
    let mut chiselc_procs = vec![];

    let mut preprocess_source = |file_path: &PathBuf| -> Result<()> {
        if optimize {
            let chiselc_proc = chiselc_spawn(file_path, file_path, entities)
                .context("Could not start `chiselc`")?;

            chiselc_procs.push(chiselc_proc);
        }

        // TODO: we need to generate indexes from all source files, not just routes
        if auto_index {
            let code = read_to_string(file_path.clone())
                .with_context(|| format!("Could not read file {}", file_path.display()))?;
            let mut indexes = parse_indexes(code, entities).with_context(|| {
                format!(
                    "Could not parse auto-indexing information from file {}",
                    file_path.display()
                )
            })?;
            index_candidates.append(&mut indexes);
        }

        Ok(())
    };

    // TODO: we need to preprocess all source files with chiselc, not just routes and events
    for route in route_map.routes.iter_mut() {
        preprocess_source(&route.file_path)?;
    }
    for topic in topic_map.topics.iter_mut() {
        preprocess_source(&topic.file_path)?;
    }

    for proc in chiselc_procs.into_iter() {
        let chiselc_output = proc
            .wait_with_output()
            .await
            .context("Could not run chiselc")?;
        ensure_success(chiselc_output).context("chiselc returned errors")?;
    }

    let bundler_input_dir =
        tempfile::tempdir().context("Could not create temporary directory for bundler input")?;
    let bundler_output_dir =
        tempfile::tempdir().context("Could not create temporary directory for bundler output")?;

    let import_fn = |path: &Path| -> Result<String> {
        path.to_str()
            .map(String::from)
            .context("Path is not valid UTF-8")
    };
    let root_code = codegen_root_module(&route_map, &topic_map, &import_fn)
        .context("Could not generate code for file-based routing and event topics")?;

    let root_path = bundler_input_dir.path().join("__root.ts");
    fs::write(&root_path, root_code)
        .context(format!("Could not write to file {}", root_path.display()))?;

    let banner = concat!(
        "import { createRequire as __createRequire } from 'chisel://deno-std/node/module.ts'; ",
        "var require = __createRequire(import.meta.url);",
        "var __filename = '__root.ts';",
    );

    let bundler_args: Vec<OsString> = vec![
        root_path.into(),
        "--bundle".into(),
        "--color=true".into(),
        "--target=esnext".into(),
        "--external:@chiselstrike".into(),
        "--external:chisel://*".into(),
        "--format=esm".into(),
        "--tree-shaking=true".into(),
        "--tsconfig=./tsconfig.json".into(),
        "--platform=node".into(),
        {
            let mut outdir = OsString::from("--outdir=");
            outdir.push(bundler_output_dir.path());
            outdir
        },
        format!("--banner:js={}", banner).into(),
    ];

    let bundler_output = esbuild(&bundler_args)?
        .wait_with_output()
        .await
        .context("Could not run esbuild")?;
    ensure_success(bundler_output)
        .context("Could not bundle routes with esbuild (using node-style modules)")?;

    let bundled_code = fs::read_to_string(bundler_output_dir.path().join("__root.js"))?;
    let modules = vec![Module {
        url: "file:///__root.ts".into(),
        code: bundled_code,
    }];

    Ok((modules, index_candidates))
}

fn esbuild<A: AsRef<OsStr>>(args: &[A]) -> Result<tokio::process::Child> {
    let command = "./node_modules/esbuild/bin/esbuild";
    let cmd = tokio::process::Command::new(command)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context(format!("Could not start `{}`", command))?;
    Ok(cmd)
}

fn npx<A: AsRef<OsStr>>(command: &'static str, args: &[A]) -> Result<tokio::process::Child> {
    let cmd = tokio::process::Command::new("npx")
        .arg(command)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context(format!("Could not start `npx {}`", command))?;
    Ok(cmd)
}

fn ensure_success(output: std::process::Output) -> Result<()> {
    if !output.status.success() {
        let out = String::from_utf8_lossy(&output.stdout);
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("{}\n{}", out, err);
    }
    Ok(())
}
