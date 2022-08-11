// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::prefix_map::PrefixMap;
use crate::types::ObjectType;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use yaml_rust::YamlLoader;

/// Different kinds of policies.
#[derive(Clone)]
pub enum Kind {
    /// How this policy transforms values read from storage.
    Transform(fn(Value) -> Value),
    /// Field is of AuthUser type and must match the user currently logged in.
    MatchLogin,
    /// Field will not be in a query's resulting json object.
    Omit,
}

#[derive(Clone)]
pub struct Policy {
    pub kind: Kind,

    /// This policy doesn't apply when the request URI matches.
    pub except_uri: regex::Regex,
}

#[derive(Clone, Default, Debug)]
pub struct FieldPolicies {
    /// Maps a field name to the transformation we apply to that field's values.
    pub transforms: HashMap<String, fn(Value) -> Value>,
    /// Names of fields that must equal the currently logged-in user.
    pub match_login: HashSet<String>,
    /// ID of the currently logged-in user.
    pub current_userid: Option<String>,
    /// Names of fields which will be excluded from query's resulting json object.
    pub omit: HashSet<String>,
}

#[derive(Clone, Default, Debug)]
pub struct UserAuthorization {
    /// A user is authorized to access a path if the username matches the regex for the longest path prefix present
    /// here.
    paths: PrefixMap<regex::Regex>,
}

impl UserAuthorization {
    /// Is this username allowed to execute the endpoint at this path?
    pub fn is_allowed(&self, username: Option<&str>, path: &str) -> bool {
        match self.paths.longest_prefix(path) {
            None => true,
            Some((_, u)) => match username {
                None => false, // Must be logged in if path specified a regex.
                Some(username) => u.is_match(username),
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
pub struct PolicySystem {
    /// Maps labels to their applicable policies.
    pub labels: HashMap<String, Policy>,
    pub user_authorization: UserAuthorization,
}

impl PolicySystem {
    /// For field of type `ty` creates field policies.
    pub fn make_field_policies(
        &self,
        user_id: &Option<String>,
        current_path: &str,
        ty: &ObjectType,
    ) -> FieldPolicies {
        let mut field_policies = FieldPolicies {
            current_userid: user_id.clone(),
            ..Default::default()
        };

        for fld in ty.user_fields() {
            for lbl in &fld.labels {
                if let Some(p) = self.labels.get(lbl) {
                    if !p.except_uri.is_match(current_path) {
                        match p.kind {
                            Kind::Transform(f) => {
                                field_policies.transforms.insert(fld.name.clone(), f);
                            }
                            Kind::MatchLogin => {
                                field_policies.match_login.insert(fld.name.clone());
                            }
                            Kind::Omit => {
                                field_policies.omit.insert(fld.name.clone());
                            }
                        }
                    }
                }
            }
        }
        field_policies
    }

    pub fn from_yaml(config: &str) -> Result<Self> {
        let mut policies = Self::default();
        let mut labels = vec![];

        let docs = YamlLoader::load_from_str(config)?;
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
                    Some("omit") => {
                        policies.labels.insert(
                            name.to_owned(),
                            Policy {
                                kind: Kind::Omit,
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
            for route in config["routes"]
                .as_vec()
                .get_or_insert(&[].into())
                .iter()
            {
                if let Some(path) = route["path"].as_str() {
                    if let Some(users) = route["users"].as_str() {
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

pub fn anonymize(_: Value) -> Value {
    // TODO: use type-specific anonymization.
    json!("xxxxx")
}
