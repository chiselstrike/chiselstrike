// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::chisel::IndexCandidate;
use crate::cmd::apply::chiselc_spawn;
use crate::cmd::apply::parse_indexes;
use crate::cmd::apply::TypeChecking;
use crate::project::read_to_string;
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use tokio::task::{spawn_blocking, JoinHandle};

pub(crate) async fn apply(
    endpoints: &[PathBuf],
    entities: &[String],
    optimize: bool,
    auto_index: bool,
    type_check: &TypeChecking,
) -> Result<(HashMap<String, String>, Vec<IndexCandidate>)> {
    let mut endpoints_req = HashMap::new();
    let mut index_candidates_req = vec![];
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

    let chiselc_futures = endpoints.iter().map(|endpoint| {
        if optimize {
            let endpoint_file_path = endpoint.clone();
            let gen_file_path = gen_dir.join(endpoint_file_path.file_name().unwrap());
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
            (Some(Box::new(future)), endpoint_file_path, import_path)
        } else {
            let path = endpoint.to_owned();
            (None, path.clone(), path)
        }
    });

    let mut bundler_file_mapping = vec![];

    let mut bundler_cmd_args = vec![];

    let bundler_input_dir = tempfile::tempdir()?;
    let bundler_output_dir = tempfile::tempdir()?;

    let bundler_input_dir_name = bundler_input_dir.path();
    let bundler_output_dir_name = bundler_output_dir.path();

    let mut idx = 0;
    for (mut chiselc_future, endpoint_file_path, import_path) in chiselc_futures {
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
        return Err(anyhow!("compiling endpoints")).with_context(|| format!("{}\n{}", out, err));
    }

    for (endpoint_name, bundler_info) in endpoints.iter().zip(bundler_file_mapping.iter()) {
        let (endpoint_file_path, idx_file_name) = bundler_info;
        let mut bundler_output_file = bundler_output_dir_name.join(&idx_file_name);
        bundler_output_file.set_extension("js");
        let code = read_to_string(bundler_output_file)?;

        endpoints_req.insert(endpoint_name.display().to_string(), code);
        if auto_index {
            let code = read_to_string(endpoint_file_path.clone())?;
            let mut indexes = parse_indexes(code, entities)?;
            index_candidates_req.append(&mut indexes);
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
    Ok((endpoints_req, index_candidates_req))
}

fn npx<A: AsRef<OsStr>, C: AsRef<OsStr>>(
    command: C,
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
            .with_context(|| "trying to execute `npx esbuild`. Is npx on your PATH?".to_string())
    })
}
