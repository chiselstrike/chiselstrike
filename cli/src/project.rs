// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::{anyhow, Context, Result};
use handlebars::Handlebars;
use serde_derive::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{stdin, Read};
use std::path::{Path, PathBuf};

const MANIFEST_FILE: &str = "Chisel.toml";
const TYPES_DIR: &str = "./models";
const ENDPOINTS_DIR: &str = "./endpoints";
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

#[derive(PartialOrd, PartialEq, Eq, Ord)]
pub(crate) struct Endpoint {
    pub(crate) name: String,
    pub(crate) file_path: PathBuf,
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
}

impl Manifest {
    pub fn models(&self) -> anyhow::Result<Vec<PathBuf>> {
        Self::dirs_to_paths(&self.models)
    }

    pub fn endpoints(&self) -> anyhow::Result<Vec<Endpoint>> {
        let mut ret = vec![];
        for dir in &self.endpoints {
            let mut paths = vec![];
            let dir = Path::new(dir);
            dir_to_paths(dir, &mut paths)?;
            let mut routes = BTreeMap::new();
            for file_path in paths {
                // file_stem returns None only if there is no file name.
                let stem = file_path.file_stem().unwrap();
                // parent returns None only for the root.
                let mut parent = file_path.parent().unwrap().to_path_buf();
                parent.push(stem);

                let name = parent.strip_prefix(&dir)?;

                if let Some(old) = routes.insert(name.to_owned(), file_path.to_owned()) {
                    anyhow::bail!("Cannot add both {} {} as routes. ChiselStrike uses filesystem-based routing, so we don't know what to do. Sorry! 🥺", old.display(), file_path.display());
                }

                let name = name
                    .to_str()
                    .ok_or_else(|| anyhow!("filename is not utf8 {:?}", name))?
                    .to_string();
                ret.push(Endpoint { file_path, name });
            }
        }
        ret.sort_unstable();
        Ok(ret)
    }

    pub fn policies(&self) -> anyhow::Result<Vec<PathBuf>> {
        Self::dirs_to_paths(&self.policies)
    }

    fn dirs_to_paths(dirs: &[String]) -> anyhow::Result<Vec<PathBuf>> {
        let mut paths = vec![];
        for dir in dirs {
            dir_to_paths(Path::new(dir), &mut paths)?
        }
        paths.sort_unstable();
        Ok(paths)
    }
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

fn ignore_path(path: &str) -> bool {
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

pub(crate) fn read_manifest() -> Result<Manifest> {
    let cwd = env::current_dir()?;
    if !Path::new(MANIFEST_FILE).exists() {
        anyhow::bail!("Could not find `{}` in `{}`. Did you forget to run `chisel init` to initialize the project?", MANIFEST_FILE, cwd.display());
    }
    let manifest = read_to_string(MANIFEST_FILE)?;
    let manifest: Manifest = match toml::from_str(&manifest) {
        Ok(manifest) => manifest,
        Err(error) => {
            anyhow::bail!(
                "Failed to parse manifest at `{}`:\n\n{}",
                cwd.join(MANIFEST_FILE).display(),
                error
            );
        }
    };
    Ok(manifest)
}

/// Opens and reads an entire file (or stdin, if filename is "-")
pub(crate) fn read_to_string<P: AsRef<Path>>(filename: P) -> anyhow::Result<String> {
    if filename.as_ref() == Path::new("-") {
        let mut s = "".to_string();
        stdin()
            .read_to_string(&mut s)
            .with_context(|| "while reading stdin".to_string())?;
        Ok(s)
    } else {
        fs::read_to_string(filename.as_ref())
            .with_context(|| format!("while reading {}", filename.as_ref().display()))
    }
}

/// Project creation options.
pub(crate) struct CreateProjectOptions {
    /// Force project creation by overwriting existing project files.
    pub(crate) force: bool,
    /// Generate example code for project.
    pub(crate) examples: bool,
}

/// Writes contents to a file in a directory.
fn write(contents: &str, dir: &Path, file: &str) -> Result<()> {
    fs::write(dir.join(file), contents).map_err(|e| e.into())
}

/// Writes "template/$file" content into $dir/$file.  The file content is read at compile time but written at
/// runtime.
macro_rules! write_template {
    ( $file:expr, $data:expr, $dir:expr ) => {{
        let mut handlebars = Handlebars::new();
        let source = include_str!(concat!("template/", $file));
        handlebars.register_template_string("t1", source)?;
        let output = handlebars.render("t1", &$data)?;
        write(&output, $dir, $file)
    }};
}

pub(crate) fn create_project(path: &Path, opts: CreateProjectOptions) -> Result<()> {
    let project_name = path.file_name().unwrap().to_str().unwrap();
    if !opts.force && project_exists(path) {
        anyhow::bail!("You cannot run `chisel init` on an existing ChiselStrike project");
    }
    fs::create_dir_all(path.join(TYPES_DIR))?;
    fs::create_dir_all(path.join(ENDPOINTS_DIR))?;
    fs::create_dir_all(path.join(POLICIES_DIR))?;
    fs::create_dir_all(path.join(VSCODE_DIR))?;

    let mut data = BTreeMap::new();
    data.insert("projectName".to_string(), project_name);
    data.insert("chiselVersion".to_string(), "latest");

    write_template!("package.json", data, path)?;
    write_template!("tsconfig.json", data, path)?;
    write_template!("Chisel.toml", data, path)?;
    // creating through chisel instead of npx: default to deno resolution
    let mut toml = String::from(include_str!("template/Chisel.toml"));
    toml.push_str("modules = \"deno\"\n");
    write(&toml, path, "Chisel.toml")?;

    write_template!("settings.json", data, &path.join(VSCODE_DIR))?;

    if opts.examples {
        write_template!("hello.ts", data, &path.join(ENDPOINTS_DIR))?;
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
