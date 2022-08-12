// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::chisel::IndexCandidate;
use crate::cmd::apply::chiselc_spawn;
use crate::cmd::apply::parse_indexes;
use crate::cmd::apply::{SourceMap, TypeChecking};
use crate::project::read_to_string;
use anyhow::{anyhow, Context, Result};
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use tokio::task::{spawn_blocking, JoinHandle};

// FIXME: merge with apply
pub(crate) async fn get_policies(policies: &[PathBuf]) -> Result<Vec<(String, String)>> {
    if policies.is_empty() {
        return Ok(vec![]);
    }

    let mut ret = vec![];

    let mut bundler_cmd_args = vec![];
    let bundler_output_dir = tempfile::tempdir()?;
    let bundler_output_dir_name = bundler_output_dir.path();

    for policy in policies.iter() {
        let bundler_entry_fname = policy.to_str().unwrap().to_owned();
        bundler_cmd_args.push(bundler_entry_fname);
    }

    bundler_cmd_args.extend_from_slice(&[
        "--bundle".to_string(),
        "--color=true".to_string(),
        "--target=esnext".to_string(),
        "--external:@chiselstrike".to_string(),
        "--format=esm".to_string(),
        "--tree-shaking=true".to_string(),
        "--tsconfig=./tsconfig.json".to_string(),
        "--platform=node".to_string(),
    ]);

    bundler_cmd_args.push(format!("--outdir={}", bundler_output_dir_name.display()));
    let cmd = npx("esbuild", &bundler_cmd_args, None);
    let res = cmd.await.unwrap()?;

    if !res.status.success() {
        let out = String::from_utf8(res.stdout).expect("command output not utf-8");
        let err = String::from_utf8(res.stderr).expect("command output not utf-8");
        return Err(anyhow!("{}\n{}", out, err))
            .context("could not bundle policies with esbuild (using node-style modules)");
    }

    for policy in policies.iter() {
        let mut bundler_output_file = bundler_output_dir_name.join(policy.file_name().unwrap());
        bundler_output_file.set_extension("js");
        let code = read_to_string(bundler_output_file)?;

        ret.push((policy.display().to_string(), code));
    }
    Ok(ret)
}

pub(crate) async fn apply(
    endpoints: &[PathBuf],
    entities: &[String],
    optimize: bool,
    auto_index: bool,
    type_check: &TypeChecking,
) -> Result<(SourceMap, Vec<IndexCandidate>)> {
    let mut sources = SourceMap::new();
    let mut index_candidates = vec![];
    let tsc = match type_check {
        TypeChecking::Yes => Some(npx(
            "tsc",
            &["--noemit", "--pretty", "--allowJs", "--checkJs"],
            None,
        )),
        TypeChecking::No => None,
    };

    let cwd = env::current_dir()?;
    let gen_dir = cwd.join(".gen");
    fs::create_dir_all(&gen_dir)?;

    let mut chiselc_futures = vec![];
    for endpoint in endpoints.iter() {
        if optimize {
            let endpoint_file_path = endpoint.clone();
            let mut components = endpoint_file_path.components();
            components.next();
            let endpoint_rel_path = components.as_path();
            anyhow::ensure!(
                endpoint_file_path.is_relative(),
                "malformed endpoint name {}. Shouldn't have reached this far",
                endpoint_file_path.display()
            );

            let gen_file_path = gen_dir.join(&endpoint_rel_path);
            let base = gen_file_path.parent().ok_or_else(|| {
                anyhow!(
                    "{} doesn't have a parent. Shouldn't have reached this far!",
                    gen_dir.display()
                )
            })?;
            fs::create_dir_all(&base)?;

            let chiselc = chiselc_spawn(
                endpoint.to_str().unwrap(),
                gen_file_path.to_str().unwrap(),
                entities,
            )
            .unwrap();
            let future = chiselc;
            let import_path = gen_file_path
                .strip_prefix(cwd.clone())
                .unwrap()
                .to_path_buf();
            chiselc_futures.push((Some(Box::new(future)), endpoint_file_path, import_path))
        } else {
            let path = endpoint.to_owned();
            chiselc_futures.push((None, path.clone(), path))
        };
    }

    let mut bundler_file_mapping = vec![];

    let mut bundler_cmd_args = vec![];

    let bundler_input_dir = tempfile::tempdir()?;
    let bundler_output_dir = tempfile::tempdir()?;

    let bundler_input_dir_name = bundler_input_dir.path();
    let bundler_output_dir_name = bundler_output_dir.path();

    let mut idx = 0;
    for (mut chiselc_future, endpoint_file_path, import_path) in chiselc_futures.into_iter() {
        idx += 1;
        if let Some(mut chiselc_future) = chiselc_future.take() {
            chiselc_future.wait().await?;
        }

        let idx_file_name = format!("{}.ts", idx);
        let file_path = bundler_input_dir_name.join(&idx_file_name);
        bundler_file_mapping.push((endpoint_file_path, idx_file_name));

        let mut file = File::create(&file_path)?;

        let mut import_path = import_path.clone();
        import_path.set_extension("");

        let code = format!(
            "import fun from \"{}/{}\";\nexport default fun",
            cwd.display(),
            import_path.display()
        );
        file.write_all(code.as_bytes())?;
        file.flush()?;
        let bundler_entry_fname = file_path.to_str().unwrap().to_owned();
        bundler_cmd_args.push(bundler_entry_fname);
    }

    bundler_cmd_args.extend_from_slice(&[
        "--bundle".to_string(),
        "--color=true".to_string(),
        "--target=esnext".to_string(),
        "--external:@chiselstrike".to_string(),
        "--format=esm".to_string(),
        "--tree-shaking=true".to_string(),
        "--tsconfig=./tsconfig.json".to_string(),
        "--platform=node".to_string(),
    ]);

    bundler_cmd_args.push(format!("--outdir={}", bundler_output_dir_name.display()));
    let cmd = npx("esbuild", &bundler_cmd_args, None);
    let res = cmd.await.unwrap()?;

    if !res.status.success() {
        let out = String::from_utf8(res.stdout).expect("command output not utf-8");
        let err = String::from_utf8(res.stderr).expect("command output not utf-8");
        return Err(anyhow!("{}\n{}", out, err))
            .context("could not bundle endpoints with esbuild (using node-style modules)");
    }

    for bundler_info in bundler_file_mapping.iter() {
        let (endpoint_file_path, idx_file_name) = bundler_info;
        let mut bundler_output_file = bundler_output_dir_name.join(&idx_file_name);
        bundler_output_file.set_extension("js");
        let code = read_to_string(bundler_output_file)?;

        sources.insert(endpoint_file_path.display().to_string(), code);
        if auto_index {
            let code = read_to_string(endpoint_file_path.clone())?;
            let mut indexes = parse_indexes(code, entities)?;
            index_candidates.append(&mut indexes);
        }
    }

    if let Some(tsc) = tsc {
        let tsc_res = tsc.await.unwrap()?;
        if !tsc_res.status.success() {
            let out = String::from_utf8(tsc_res.stdout).expect("command output not utf-8");
            let err = String::from_utf8(tsc_res.stderr).expect("command output not utf-8");
            anyhow::bail!("{}\n{}", out, err);
        }
    }
    Ok((sources, index_candidates))
}

fn npx<A: AsRef<OsStr>>(
    command: &'static str,
    args: &[A],
    stdin: Option<std::process::ChildStdout>,
) -> JoinHandle<Result<std::process::Output>> {
    let mut cmd = std::process::Command::new("npx");
    cmd.arg(command).args(args);

    if let Some(stdin) = stdin {
        cmd.stdin(stdin);
    }

    spawn_blocking(move || {
        cmd.output()
            .with_context(|| format!("could not execute `npx {}`. Is npx on your PATH?", command))
    })
}
