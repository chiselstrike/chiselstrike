use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{events::FileTopicMap, routes::FileRouteMap};
use anyhow::{anyhow, Context, Result};

pub(crate) fn create_tmp_route_files(
    mut file_map: FileRouteMap,
    gen_dir: &Path,
) -> Result<FileRouteMap> {
    let cwd = env::current_dir()?;
    for route in file_map.routes.iter_mut() {
        copy_source(&cwd, &mut route.file_path, gen_dir)?;
    }
    Ok(file_map)
}

pub(crate) fn create_tmp_topic_files(
    mut file_map: FileTopicMap,
    gen_dir: &Path,
) -> Result<FileTopicMap> {
    let cwd = env::current_dir()?;
    for route in file_map.topics.iter_mut() {
        copy_source(&cwd, &mut route.file_path, gen_dir)?;
    }
    Ok(file_map)
}

fn copy_source(cwd: &PathBuf, file_path: &mut PathBuf, gen_dir: &Path) -> Result<()> {
    let file_rel_path = file_path
        .strip_prefix(cwd)
        .with_context(|| format!("File {} is not a part of this project", file_path.display(),))?;

    // NOTE: this a horrible hack to make relative imports work
    // it is common that file "routes/books.ts" imports "models/Book.ts" using
    // "../models/Book.ts". to make this work with the bundler, we must place the generated
    // file into ".gen/books.ts".
    let mut file_rel_components = file_rel_path.components();
    file_rel_components.next();
    let file_rel_path = file_rel_components.as_path();

    let gen_file_path = gen_dir.join(file_rel_path);
    let gen_parent_path = gen_file_path.parent().ok_or_else(|| {
        anyhow!(
            "{} doesn't have a parent. Shouldn't have reached this far!",
            gen_dir.display()
        )
    })?;
    fs::create_dir_all(gen_parent_path)
        .with_context(|| format!("Could not create directory {}", gen_parent_path.display()))?;
    fs::copy(&file_path, &gen_file_path).context("failed to copy source file to .gen directory")?;

    // use the chiselc-processed file instead of the original file in the route map
    *file_path = gen_file_path;
    Ok(())
}
