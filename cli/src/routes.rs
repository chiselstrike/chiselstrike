// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// The set of file-based routes extracted from the filesystem.
///
/// We generate a TypeScript `RouteMap` from this struct.
#[derive(Debug, Default)]
pub(crate) struct FileRouteMap {
    pub routes: Vec<FileRoute>,
}

impl FileRouteMap {
    fn add_route(
        &mut self,
        file_path: PathBuf,
        path_pattern: String,
        legacy_file_name: Option<String>,
    ) {
        self.routes.push(FileRoute {
            file_path,
            path_pattern,
            legacy_file_name,
        });
    }
}

/// A file-based route in [`FileRouteMap`].
///
/// Files are mapped to routes as follows:
///
/// - file `user/by-id.ts' -> route `"/user/by-id"`
/// - file `user/index.ts' -> route `"/user"`
/// - file `user/[id].ts' -> route `"/user/:id"` (matched group)
/// - file `user/[id]/posts.ts' -> route `"/user/:id/posts"`
/// - file `user/_root.ts` -> route `"/user"`, and other files in the directory `user/` are ignored
#[derive(Debug)]
pub(crate) struct FileRoute {
    /// Absolute path to the file with the route.
    pub file_path: PathBuf,
    /// URL Pattern path for this route.
    pub path_pattern: String,
    /// Legacy: relative path to the file without extension. Remove this when `RouteMap.convert()`
    /// no longer needs it to emulate the deprecated field `ChiselRequest.endpoint`.
    pub legacy_file_name: Option<String>,
}

pub(crate) fn build_file_route_map(
    base_dir: &Path,
    route_dirs: &[PathBuf],
) -> Result<FileRouteMap> {
    let mut route_map = FileRouteMap::default();
    for route_dir in route_dirs.iter() {
        let route_dir = base_dir.join(route_dir);
        let route_dir = fs::canonicalize(&route_dir)
            .with_context(|| format!("Could not canonicalize path {}", route_dir.display()))?;
        walk_dir(&mut route_map, &route_dir, &route_dir, "")
            .with_context(|| format!("Could not read routes from {}", route_dir.display()))?;
    }
    route_map
        .routes
        .sort_unstable_by_key(|route| route.file_path.clone());
    Ok(route_map)
}

fn walk_dir(
    route_map: &mut FileRouteMap,
    route_dir: &Path,
    dir_path: &Path,
    path_pattern: &str,
) -> Result<()> {
    let root_ts_path = dir_path.join("_root.ts");
    let root_is_file = try_is_file(&root_ts_path).with_context(|| {
        format!(
            "Could not determine whether {} exists",
            root_ts_path.display()
        )
    })?;
    if root_is_file {
        route_map.add_route(root_ts_path, path_pattern.into(), None);
        return Ok(());
    }

    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        walk_dir_entry(route_map, route_dir, &entry, path_pattern)?;
    }
    Ok(())
}

fn walk_dir_entry(
    route_map: &mut FileRouteMap,
    route_dir: &Path,
    entry: &fs::DirEntry,
    path_pattern: &str,
) -> Result<()> {
    let entry_name = entry.file_name();
    let entry_name = entry_name
        .to_str()
        .with_context(|| format!("Cannot convert file name {:?} to UTF-8", entry.file_name()))?;
    if entry_name.starts_with('_') || entry_name.starts_with('.') {
        return Ok(());
    }

    let entry_path = entry.path();
    // don't use `entry.file_type()`, because we want to follow symlinks
    let metadata = fs::metadata(&entry_path)
        .with_context(|| format!("Could not read metadata of {}", entry_path.display()))?;
    if metadata.is_file() {
        if let Some(stem) = entry_name.strip_suffix(".ts") {
            let legacy_file_name = get_legacy_file_name(route_dir, &entry_path);
            route_map.add_route(
                entry_path,
                match stem {
                    "index" => path_pattern.to_string(),
                    stem => format!("{}/{}", path_pattern, stem_to_pattern(stem)),
                },
                legacy_file_name,
            );
        } else if entry_name.ends_with(".js") {
            bail!(
                "Found file {}, but only TypeScript files (.ts) are supported",
                entry_path.display()
            );
        }
    } else if metadata.is_dir() {
        let dir_path_pattern = format!("{}/{}", path_pattern, stem_to_pattern(entry_name));
        walk_dir(route_map, route_dir, &entry_path, &dir_path_pattern)
            .with_context(|| format!("Could not read routes from {}", entry_path.display()))?;
    }

    Ok(())
}

fn stem_to_pattern(stem: &str) -> String {
    match stem.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
        Some(group_name) => format!(":{}", group_name),
        None => stem.to_string(),
    }
}

fn try_is_file(path: &Path) -> Result<bool> {
    use std::io::ErrorKind;
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(err) => match err.kind() {
            ErrorKind::NotFound => Ok(false),
            _ => Err(anyhow!(err)),
        },
    }
}

fn get_legacy_file_name(route_dir: &Path, file_path: &Path) -> Option<String> {
    let relative_path = file_path.strip_prefix(route_dir).ok()?;
    let parent = relative_path.parent()?;
    let stem = relative_path.file_stem()?;
    parent.join(stem).to_str().map(str::to_string)
}
