// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(crate) struct Policy {
    /// How this policy transforms values read from storage.
    pub(crate) transform: fn(Value) -> Value,

    /// This policy doesn't apply when the request URI matches.
    pub(crate) except_uri: regex::Regex,
}

/// Maps labels to their applicable policies.
pub(crate) type LabelPolicies = HashMap<String, Policy>;

/// Maps a field name to the transformation we apply to that field's values.
pub(crate) type FieldPolicies = HashMap<String, fn(Value) -> Value>;

#[derive(Clone, Default)]
pub(crate) struct UserAuthorization {
    /// A user is authorized to access a path if the username matches the regex for the longest path prefix present
    /// here.
    paths: Vec<(PathBuf, regex::Regex)>, // Reverse-sorted.
}

impl UserAuthorization {
    /// Is this username allowed to execute the endpoint at this path?
    pub fn is_allowed<S: AsRef<Path>>(&self, username: Option<String>, path: S) -> bool {
        let path = path.as_ref();
        for p in &self.paths {
            if path.starts_with(&p.0) {
                return match username {
                    None => false, // Must be logged in if path specified a regex.
                    Some(username) => p.1.is_match(&username),
                };
            }
        }
        true
    }

    /// Authorizes users matching a regex to execute any endpoint under this path.  Longer paths override existing
    /// prefixes.  Error if this same path has already been added.
    pub fn add(&mut self, path: &str, users: regex::Regex) -> Result<(), anyhow::Error> {
        let path: PathBuf = path.into();
        match self.paths.binary_search_by(|p| path.cmp(&p.0)) {
            Ok(_) => anyhow::bail!("Repeated path in user authorization: {:?}", path),
            Err(pos) => {
                self.paths.insert(pos, (path, users));
                Ok(())
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct Policies {
    pub(crate) labels: LabelPolicies,
    pub(crate) user_authorization: UserAuthorization,
}

impl Policies {
    pub(crate) fn new() -> Self {
        Self {
            labels: LabelPolicies::default(),
            user_authorization: UserAuthorization::default(),
        }
    }
}

pub(crate) fn anonymize(_: Value) -> Value {
    // TODO: use type-specific anonymization.
    json!("xxxxx")
}
