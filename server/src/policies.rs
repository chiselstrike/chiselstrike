// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::prefix_map::PrefixMap;
use crate::types::ObjectType;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use yaml_rust::YamlLoader;

/// Different kinds of policies.
#[derive(Clone)]
pub(crate) enum Kind {
    /// How this policy transforms values read from storage.
    Transform(fn(Value) -> Value),
    /// Field is of OAuthUser type and must match the user currently logged in.
    MatchLogin,
}

#[derive(Clone)]
pub(crate) struct Policy {
    pub(crate) kind: Kind,

    /// This policy doesn't apply when the request URI matches.
    pub(crate) except_uri: regex::Regex,
}

/// Maps labels to their applicable policies.
pub(crate) type LabelPolicies = HashMap<String, Policy>;

#[derive(Clone, Default, Debug)]
pub(crate) struct FieldPolicies {
    /// Maps a field name to the transformation we apply to that field's values.
    pub(crate) transforms: HashMap<String, fn(Value) -> Value>,
    /// Names of fields that must equal the currently logged-in user.
    pub(crate) match_login: HashSet<String>,
    /// ID of the currently logged-in user.
    pub(crate) current_userid: Option<String>,
}

#[derive(Clone, Default, Debug)]
pub(crate) struct UserAuthorization {
    /// A user is authorized to access a path if the username matches the regex for the longest path prefix present
    /// here.
    paths: PrefixMap<regex::Regex>,
}

impl UserAuthorization {
    /// Is this username allowed to execute the endpoint at this path?
    pub fn is_allowed(&self, username: Option<String>, path: &Path) -> bool {
        match self.paths.longest_prefix(path) {
            None => true,
            Some((_, u)) => match username {
                None => false, // Must be logged in if path specified a regex.
                Some(username) => u.is_match(&username),
            },
        }
    }

    /// Authorizes users matching a regex to execute any endpoint under this path.  Longer paths override existing
    /// prefixes.  Error if this same path has already been added.
    pub fn add(&mut self, path: &str, users: regex::Regex) -> Result<()> {
        if self.paths.insert(path.into(), users).is_some() {
            anyhow::bail!("Repeated path in user authorization: {:?}", path);
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub(crate) struct VersionPolicy {
    pub(crate) labels: LabelPolicies,
    pub(crate) user_authorization: UserAuthorization,
}

#[derive(Clone, Default)]
pub(crate) struct Policies {
    pub(crate) versions: HashMap<String, VersionPolicy>,
}

impl Policies {
    pub(crate) fn add_from_yaml<K: ToString, Y: AsRef<str>>(
        &mut self,
        version: K,
        yaml: Y,
    ) -> Result<()> {
        let v = VersionPolicy::from_yaml(yaml)?;
        self.versions.insert(version.to_string(), v);
        Ok(())
    }

    /// Adds the current policies on ty's fields.
    pub(crate) fn add_field_policies(
        &self,
        ty: &ObjectType,
        policies: &mut FieldPolicies,
        current_path: &str,
    ) {
        if let Some(version) = self.versions.get(&ty.api_version) {
            for fld in ty.user_fields() {
                for lbl in &fld.labels {
                    if let Some(p) = version.labels.get(lbl) {
                        if !p.except_uri.is_match(current_path) {
                            match p.kind {
                                Kind::Transform(f) => {
                                    policies.transforms.insert(fld.name.clone(), f);
                                }
                                Kind::MatchLogin => {
                                    policies.match_login.insert(fld.name.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

impl VersionPolicy {
    pub(crate) fn from_yaml<S: AsRef<str>>(config: S) -> Result<Self> {
        let mut policies = Self::default();
        let mut labels = vec![];

        let docs = YamlLoader::load_from_str(config.as_ref())?;
        for config in docs.iter() {
            for label in config["labels"].as_vec().get_or_insert(&[].into()).iter() {
                let name = label["name"].as_str().ok_or_else(|| {
                    anyhow::anyhow!("couldn't parse yaml: label without a name: {:?}", label)
                })?;

                labels.push(name.to_owned());
                debug!("Applying policy for label {:?}", name);
                let pattern = label["except_uri"].as_str().unwrap_or("^$"); // ^$ never matches; each path has at least a '/' in it.

                match label["transform"].as_str() {
                    Some("anonymize") => {
                        policies.labels.insert(
                            name.to_owned(),
                            Policy {
                                kind: Kind::Transform(crate::policies::anonymize),
                                except_uri: regex::Regex::new(pattern)?,
                            },
                        );
                    }
                    Some("match_login") => {
                        policies.labels.insert(
                            name.to_owned(),
                            Policy {
                                kind: Kind::MatchLogin,
                                except_uri: regex::Regex::new(pattern)?,
                            },
                        );
                    }
                    Some(x) => {
                        anyhow::bail!("unknown transform: {} for label {}", x, name);
                    }
                    None => {}
                };
            }
            for endpoint in config["endpoints"]
                .as_vec()
                .get_or_insert(&[].into())
                .iter()
            {
                if let Some(path) = endpoint["path"].as_str() {
                    if let Some(users) = endpoint["users"].as_str() {
                        policies
                            .user_authorization
                            .add(path, regex::Regex::new(users)?)?;
                    }
                }
            }
        }
        Ok(policies)
    }
}

pub(crate) fn anonymize(_: Value) -> Value {
    // TODO: use type-specific anonymization.
    json!("xxxxx")
}
