// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::{Context, Result};
use handlebars::Handlebars;
use serde_derive::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::fmt::Write;
use std::fs;
use std::io::{stdin, ErrorKind, Read};
use std::path::{Path, PathBuf};
use utils::without_extension;

const MANIFEST_FILE: &str = "Chisel.toml";
const TYPES_DIR: &str = "./models";
const ENDPOINTS_DIR: &str = "./endpoints";
const LIB_DIR: &str = "./lib";
const POLICIES_DIR: &str = "./policies";
const VSCODE_DIR: &str = "./.vscode/";

#[derive(Deserialize, PartialEq)]
pub(crate) enum Module {
    #[serde(rename = "node")]
    Node,
    #[serde(rename = "deno")]
    Deno,
}

impl Default for Module {
    fn default() -> Self {
        Module::Node
    }
}

#[derive(Deserialize, PartialEq)]
pub(crate) enum Optimize {
    #[serde(rename = "yes")]
    Yes,
    #[serde(rename = "no")]
    No,
}

impl Default for Optimize {
    fn default() -> Self {
        Optimize::Yes
    }
}

#[derive(Deserialize, PartialEq)]
pub(crate) enum AutoIndex {
    #[serde(rename = "yes")]
    Yes,
    #[serde(rename = "no")]
    No,
}

impl Default for AutoIndex {
    fn default() -> Self {
        AutoIndex::No
    }
}

/// Manifest defines the files that describe types, endpoints, and policies.
///
/// The manifest is a high-level declaration of application behavior.
/// The individual definitions are passed to `chiseld`, which processes them
/// accordingly. For example, type definitions are imported as types and
/// endpoints are made executable via Deno.
#[derive(Deserialize)]
pub(crate) struct Manifest {
    /// Vector of directories to scan for model definitions.
    pub(crate) models: Vec<String>,
    /// Vector of directories to scan for endpoint definitions.
    pub(crate) endpoints: Vec<String>,
    /// Vector of directories to scan for policy definitions.
    pub(crate) policies: Vec<String>,
    /// Whether to use deno-style or node-style modules
    #[serde(default)]
    pub(crate) modules: Module,
    /// Enable or disable query optimization with the `chiselc` compiler.
    #[serde(default)]
    pub(crate) optimize: Optimize,
    /// Enable or disable auto-indexing.
    #[serde(default)]
    pub(crate) auto_index: AutoIndex,
}

impl Manifest {
    pub fn models(&self) -> anyhow::Result<Vec<PathBuf>> {
        Self::dirs_to_paths(&self.models)
    }

    pub fn endpoints(&self) -> anyhow::Result<Vec<PathBuf>> {
        let ret = Self::dirs_to_paths(&self.endpoints)?;
        if let Some((a, b)) = check_duplicates(&ret) {
            anyhow::bail!("Cannot add both {} {} as routes. ChiselStrike uses filesystem-based routing, so we don't know what to do. Sorry! 🥺", a, b);
        }
        Ok(ret)
    }

    pub fn policies(&self) -> anyhow::Result<Vec<PathBuf>> {
        Self::dirs_to_paths(&self.policies)
    }

    fn dirs_to_paths(dirs: &[String]) -> anyhow::Result<Vec<PathBuf>> {
        // sucks to do this for all invocations but keeps things simple
        let me = Path::new("./").canonicalize()?;
        let mut paths = vec![];
        for dir in dirs {
            let p = Path::new(dir);
            anyhow::ensure!(
                p.is_relative(),
                "{} is not relative to the current tree",
                dir
            );
            let p = p.canonicalize().or_else(|x| match x.kind() {
                ErrorKind::NotFound => Ok(PathBuf::new()),
                _ => Err(x),
            })?;

            if p.as_os_str().is_empty() {
                continue;
            }
            anyhow::ensure!(
                me != p && p.starts_with(&me),
                "{} has to be a subdirectory of the current directory",
                dir
            );
            dir_to_paths(Path::new(dir), &mut paths)?
        }
        paths.sort_unstable();
        Ok(paths)
    }
}

fn check_duplicates(source_files: &[PathBuf]) -> Option<(String, String)> {
    // Check for duplicated endpoints now since otherwise TSC
    // reports the issue and we can produce a better diagnostic
    // than TSC.
    let i = source_files.iter();
    for (a, b) in i.clone().zip(i.skip(1)) {
        let a = &a.display().to_string();
        let b = &b.display().to_string();
        if without_extension(a) == without_extension(b) {
            return Some((a.to_string(), b.to_string()));
        }
    }
    None
}

fn dir_to_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for dentry in read_dir(dir)? {
        let dentry = dentry?;
        let path = dentry.path();
        if dentry.file_type()?.is_dir() {
            dir_to_paths(&path, paths)?;
        } else if !dentry.file_name().to_str().map_or(false, ignore_path) {
            // files with names that can't be converted wtih to_str() or that start with . are
            // ignored
            paths.push(path);
        }
    }
    Ok(())
}

pub fn ignore_path(path: &str) -> bool {
    if path.starts_with('.') {
        return true;
    }
    if path.ends_with('~') {
        return true;
    }
    if path.starts_with('#') && path.ends_with('#') {
        // Emacs auto-save files.
        return true;
    }
    false
}

fn read_dir<P: AsRef<Path>>(dir: P) -> anyhow::Result<Vec<std::io::Result<fs::DirEntry>>> {
    match fs::read_dir(dir.as_ref()) {
        Ok(x) => Ok(x.collect()),
        Err(x) => {
            if x.kind() == std::io::ErrorKind::NotFound {
                Ok(vec![])
            } else {
                Err(x)
            }
        }
    }
    .with_context(|| format!("Could not open {}", dir.as_ref().display()))
}

fn read_manifest_from(dir: &Path) -> Result<Manifest> {
    let file = dir.join(MANIFEST_FILE);

    if !file.exists() {
        anyhow::bail!("Could not find `{}` in `{}`. Did you forget to run `chisel init` to initialize the project?", MANIFEST_FILE, dir.display());
    }
    let manifest = read_to_string(&file)?;
    let manifest: Manifest = match toml::from_str(&manifest) {
        Ok(manifest) => manifest,
        Err(error) => {
            anyhow::bail!(
                "Failed to parse manifest at `{}`:\n\n{}",
                file.display(),
                error
            );
        }
    };
    Ok(manifest)
}

pub(crate) fn read_manifest() -> Result<Manifest> {
    let cwd = env::current_dir()?;
    read_manifest_from(&cwd)
}

/// Opens and reads an entire file (or stdin, if filename is "-")
pub(crate) fn read_to_string<P: AsRef<Path>>(filename: P) -> anyhow::Result<String> {
    if filename.as_ref() == Path::new("-") {
        let mut s = "".to_string();
        stdin()
            .read_to_string(&mut s)
            .context("could not read stdin")?;
        Ok(s)
    } else {
        fs::read_to_string(filename.as_ref())
            .with_context(|| format!("could not read {}", filename.as_ref().display()))
    }
}

/// Project creation options.
pub(crate) struct CreateProjectOptions {
    /// Force project creation by overwriting existing project files.
    pub(crate) force: bool,
    /// Generate example code for project.
    pub(crate) examples: bool,
    /// Enable the optimizer.
    pub(crate) optimize: bool,
    /// Enable auto-indexing.
    pub(crate) auto_index: bool,
}

/// Writes contents to a file in a directory.
fn write(contents: &str, dir: &Path, file: &str) -> Result<()> {
    fs::write(dir.join(file), contents).map_err(|e| e.into())
}

/// Writes "template/$from" content into $dir/$to.  The file content is read at compile time but written at
/// runtime.
macro_rules! write_template {
    ( $from:expr, $to:expr, $data:expr, $dir:expr ) => {{
        let mut handlebars = Handlebars::new();
        let source = include_str!(concat!("template/", $from));
        handlebars.register_template_string("t1", source)?;
        let output = handlebars.render("t1", &$data)?;
        write(&output, $dir, $to)
    }};
}

pub(crate) fn create_project(path: &Path, opts: CreateProjectOptions) -> Result<()> {
    let project_name = path.file_name().unwrap().to_str().unwrap();
    if !opts.force && project_exists(path) {
        anyhow::bail!("You cannot run `chisel init` on an existing ChiselStrike project");
    }
    fs::create_dir_all(path.join(TYPES_DIR))?;
    fs::create_dir_all(path.join(ENDPOINTS_DIR))?;
    fs::create_dir_all(path.join(LIB_DIR))?;
    fs::create_dir_all(path.join(POLICIES_DIR))?;
    fs::create_dir_all(path.join(VSCODE_DIR))?;

    let mut data = BTreeMap::new();
    data.insert("projectName".to_string(), project_name);
    data.insert("chiselVersion".to_string(), "latest");

    write_template!("package.json", "package.json", data, path)?;
    write_template!("tsconfig.json", "tsconfig.json", data, path)?;
    write_template!("Chisel.toml", "Chisel.toml", data, path)?;
    write_template!("gitignore", ".gitignore", data, path)?;
    // creating through chisel instead of npx: default to deno resolution
    let mut toml = String::from(include_str!("template/Chisel.toml"));
    toml.push_str("modules = \"deno\"\n");
    writeln!(
        toml,
        "optimize = \"{}\"",
        if opts.optimize { "yes" } else { "no" }
    )
    .unwrap();
    writeln!(
        toml,
        "auto_index = \"{}\"",
        if opts.auto_index { "yes" } else { "no" }
    )
    .unwrap();
    write(&toml, path, "Chisel.toml")?;

    write_template!(
        "settings.json",
        "settings.json",
        data,
        &path.join(VSCODE_DIR)
    )?;

    if opts.examples {
        write_template!("hello.ts", "hello.ts", data, &path.join(ENDPOINTS_DIR))?;
    }
    println!("Created ChiselStrike project in {}", path.display());
    Ok(())
}

pub(crate) fn project_exists(path: &Path) -> bool {
    path.join(Path::new(MANIFEST_FILE)).exists()
        || path.join(Path::new(TYPES_DIR)).exists()
        || path.join(Path::new(ENDPOINTS_DIR)).exists()
        || path.join(Path::new(POLICIES_DIR)).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn gen_manifest(toml: &str) -> TempDir {
        let tmp_dir = TempDir::new().unwrap();
        let dir = tmp_dir.path();
        std::fs::write(dir.join(MANIFEST_FILE), toml.as_bytes()).unwrap();
        std::fs::create_dir(dir.join("./policies")).unwrap();
        std::fs::create_dir(dir.join("./endpoints")).unwrap();
        std::fs::create_dir(dir.join("./models")).unwrap();
        tmp_dir
    }

    #[test]
    fn parse_works() {
        let d = gen_manifest(
            r#"
models = ["models"]
endpoints = ["endpoints"]
policies = ["policies"]
"#,
        );
        println!("reading {:?}", std::env::current_dir());
        let m = read_manifest_from(d.path()).unwrap();
        m.models().unwrap();
        m.policies().unwrap();
        m.endpoints().unwrap();
    }

    #[should_panic(expected = "is not relative")]
    #[test]
    fn parse_absolute_fails() {
        let d = gen_manifest(
            r#"
models = ["/models/models"]
endpoints = ["endpoints"]
policies = ["policies"]
"#,
        );
        let m = read_manifest_from(d.path()).unwrap();
        m.models().unwrap();
        m.policies().unwrap();
        m.endpoints().unwrap();
    }

    #[should_panic(expected = "has to be a subdirectory")]
    #[test]
    fn parse_curr_dir_fails() {
        let d = gen_manifest(
            r#"
models = ["./"]
endpoints = ["endpoints"]
policies = ["policies"]
"#,
        );
        let m = read_manifest_from(d.path()).unwrap();
        m.models().unwrap();
        m.policies().unwrap();
        m.endpoints().unwrap();
    }

    #[should_panic(expected = "has to be a subdirectory")]
    #[test]
    fn parse_non_subdir_dir_fails() {
        let d = gen_manifest(
            r#"
models = ["../"]
endpoints = ["endpoints"]
policies = ["policies"]
"#,
        );
        let m = read_manifest_from(d.path()).unwrap();
        m.models().unwrap();
        m.policies().unwrap();
        m.endpoints().unwrap();
    }
}
