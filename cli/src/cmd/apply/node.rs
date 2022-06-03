// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::chisel::{EndPointCreationRequest, IndexCandidate};
use crate::cmd::apply::chiselc_spawn;
use crate::cmd::apply::parse_indexes;
use crate::cmd::apply::TypeChecking;
use crate::project::read_to_string;
use crate::project::Endpoint;
use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs::{self};
use std::io::Write;
use tempfile::Builder;
use tokio::task::{spawn_blocking, JoinHandle};

pub(crate) async fn apply(
    endpoints: &[Endpoint],
    entities: &[String],
    optimize: bool,
    auto_index: bool,
    type_check: &TypeChecking,
) -> Result<(Vec<EndPointCreationRequest>, Vec<IndexCandidate>)> {
    let mut endpoints_req = vec![];
    let mut index_candidates_req = vec![];
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
    let gen_dir = cwd.join(".gen");
    fs::create_dir_all(&gen_dir)?;

    let chiselc_futures: Vec<(_, _, _)> = endpoints
        .iter()
        .map(|endpoint| {
            if optimize {
                let endpoint_file_path = endpoint.file_path.clone();
                let gen_file_path = gen_dir.join(endpoint_file_path.file_name().unwrap());
                let chiselc = chiselc_spawn(
                    endpoint.file_path.to_str().unwrap(),
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
                let path = endpoint.file_path.to_owned();
                (None, path.clone(), path)
            }
        })
        .collect();

    for (mut chiselc_future, endpoint_file_path, import_path) in chiselc_futures {
        if let Some(mut chiselc_future) = chiselc_future.take() {
            chiselc_future.wait().await?;
        }
        let out = Builder::new().suffix(".ts").tempfile()?;
        let bundler_output_file = out.path().to_str().unwrap().to_owned();

        let mut f = Builder::new().suffix(".ts").tempfile()?;
        let inner = f.as_file_mut();
        let mut import_path = import_path.clone();
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
        endpoint_futures.push((endpoint_file_path.clone(), bundler_output_file.clone(), cmd));
    }

    for (endpoint, execution) in endpoints.iter().zip(endpoint_futures.into_iter()) {
        let (endpoint_file_path, bundler_output_file, res) = execution;
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
            code: code.clone(),
        });
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
