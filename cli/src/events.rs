// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{bail, Context, Result};
use guard::guard;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

/// The set of Kafka topics extracted from the filesystem.
///
/// We generate a TypeScript `TopicMap` from this struct.
#[derive(Debug, Default)]
pub(crate) struct FileTopicMap {
    pub topics: Vec<FileTopic>,
}

/// A file with event handler for a Kafka topic.
#[derive(Debug)]
pub(crate) struct FileTopic {
    /// Absolute path to the file with the event handler.
    pub file_path: PathBuf,
    /// Kafka topic for this handler.
    pub topic: String,
}

pub(crate) fn build_file_topic_map(
    base_dir: &Path,
    event_dirs: &[PathBuf],
) -> Result<FileTopicMap> {
    let mut topic_map = FileTopicMap::default();

    for event_dir in event_dirs.iter() {
        let event_dir = base_dir.join(event_dir);
        let event_dir = fs::canonicalize(&event_dir)
            .with_context(|| format!("Could not canonicalize path {}", event_dir.display()))?;

        for entry in fs::read_dir(event_dir)? {
            let entry = entry?;
            let entry_path = entry.path();

            if entry_path.extension() == Some(OsStr::new("ts")) {
                guard! {let Some(stem) = entry_path.file_stem() else {
                    continue
                }};

                let topic = stem
                    .to_str()
                    .with_context(|| {
                        format!("Filename of {} is not in UTF-8", entry_path.display())
                    })?
                    .to_string();
                topic_map.topics.push(FileTopic {
                    file_path: entry_path,
                    topic,
                });
            } else if entry_path.extension() == Some(OsStr::new("js")) {
                bail!(
                    "Found file {}, but only TypeScript files (.ts) are supported as event handlers",
                    entry_path.display(),
                );
            }
        }
    }

    Ok(topic_map)
}
