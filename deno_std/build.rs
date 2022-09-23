// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{anyhow, ensure, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::{env, fs};

struct Module {
    path_segments: Vec<String>,
    transpiled_text: String,
}

fn main() -> Result<()> {
    let crate_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let deno_std_dir = crate_dir.join("../third_party/deno_std");

    let mut modules = Vec::new();
    transpile_dir(&deno_std_dir, &[], &mut modules)?;
    ensure!(
        !modules.is_empty(),
        "No TypeScript/JavaScript source files were found in {}. Did you update the git submodule?",
        deno_std_dir.display(),
    );

    let mut gen_lines = Vec::<String>::new();
    gen_lines.push("lazy_static! {".into());
    gen_lines
        .push("    pub static ref SOURCES_JS: HashMap<&'static str, &'static str> = vec![".into());
    for (i, module) in modules.iter().enumerate() {
        let js_file_name = format!("transpiled_{}.js", i);
        let js_path = Path::new(&env::var_os("OUT_DIR").unwrap()).join(&js_file_name);
        fs::write(js_path, &module.transpiled_text)?;

        let url_path = module.path_segments.join("/");
        gen_lines.push(format!(
            "        ({:?}, include_str!(concat!(env!(\"OUT_DIR\"), \"/\", {:?}))),",
            url_path, js_file_name,
        ));
    }
    gen_lines.push("    ].into_iter().collect();".into());
    gen_lines.push("}".into());

    let gen_path = Path::new(&env::var_os("OUT_DIR").unwrap()).join("SOURCES_JS.rs");
    let gen_text = gen_lines.join("\n");
    fs::write(gen_path, gen_text)?;

    Ok(())
}

lazy_static! {
    static ref SKIP_DIRS: HashSet<&'static str> = vec![
        "examples",
        "encoding/testdata",
        "node/integrationtest",
        "node/_tools/test",
        "node/testdata",
    ]
    .into_iter()
    .collect();
    static ref SKIP_FILE: Regex = Regex::new(r"_test\.ts$").unwrap();
}

fn transpile_dir(
    dir_path: &Path,
    dir_path_segments: &[String],
    out_modules: &mut Vec<Module>,
) -> Result<()> {
    println!("cargo:rerun-if-changed={}", dir_path.display());

    let dir_url_path = dir_path_segments.join("/");
    if SKIP_DIRS.contains(dir_url_path.as_str()) {
        return Ok(());
    }

    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let file_name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow!("file name is not utf-8"))?;

        if SKIP_FILE.is_match(&file_name) {
            continue;
        }

        let file_type = entry.file_type()?;

        let mut entry_path_segments: Vec<_> = dir_path_segments.into();
        entry_path_segments.push(file_name.clone());

        if file_type.is_dir() {
            transpile_dir(&entry.path(), &entry_path_segments, out_modules)?;
        } else if file_type.is_file() {
            let media_type = if file_name.ends_with(".ts") {
                Some(deno_ast::MediaType::TypeScript)
            } else if file_name.ends_with(".js") {
                Some(deno_ast::MediaType::JavaScript)
            } else if file_name.ends_with(".mjs") {
                Some(deno_ast::MediaType::Mjs)
            } else {
                None
            };

            if let Some(media_type) = media_type {
                let module = transpile_file(&entry.path(), entry_path_segments, media_type)?;
                out_modules.push(module);
            }
        }
    }
    Ok(())
}

fn transpile_file(
    path: &Path,
    path_segments: Vec<String>,
    media_type: deno_ast::MediaType,
) -> Result<Module> {
    println!("cargo:rerun-if-changed={}", path.display());
    let source_bytes = fs::read(path)?;
    let source_text = String::from_utf8(source_bytes)?;
    let text_info = deno_ast::SourceTextInfo::from_string(source_text);

    let parse_params = deno_ast::ParseParams {
        specifier: format!("file://{}", path.display()),
        text_info,
        media_type,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    };
    let parsed_source = deno_ast::parse_module(parse_params)?;

    let emit_options = deno_ast::EmitOptions::default();
    let transpiled_source = parsed_source.transpile(&emit_options)?;
    Ok(Module {
        path_segments,
        transpiled_text: transpiled_source.text,
    })
}
