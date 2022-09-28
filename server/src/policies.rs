// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::datastore::value::EntityValue;
use crate::prefix_map::PrefixMap;
use crate::types::ObjectType;
use crate::JsonObject;
use anyhow::Result;
use chiselc::parse::ParserContext;
use hyper::http;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Different kinds of policies.
#[derive(Clone)]
pub enum Kind {
    /// How this policy transforms values read from storage.
    Transform(fn(EntityValue) -> EntityValue),
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
    pub transforms: HashMap<String, fn(EntityValue) -> EntityValue>,
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

/// Describes secret-based authorization.  An endpoint request will only be allowed if it includes a header
/// specified in this struct.
#[derive(Clone, Default, Debug)]
pub struct SecretAuthorization {
    /// A request can access an endpoint if it includes a header required by the longest path prefix.
    paths: PrefixMap<RequiredHeader>,
}

impl SecretAuthorization {
    /// Is a request with these headers allowed to execute the endpoint at this path?
    pub fn is_allowed(&self, req: &http::request::Parts, secrets: &JsonObject, path: &str) -> bool {
        match self.paths.longest_prefix(path) {
            None => true,
            Some((
                _,
                RequiredHeader {
                    methods: Some(v), ..
                },
            )) if !v.contains(&req.method) => true,
            Some((
                _,
                RequiredHeader {
                    header_name,
                    secret_name,
                    ..
                },
            )) => {
                let secret_value = match secrets.get(secret_name).cloned() {
                    None => return false, // No expected header value provided => nothing can match.
                    Some(serde_json::Value::String(s)) => s,
                    _ => {
                        warn!("Header auth failed because secret {secret_name} isn't a string");
                        return false;
                    }
                };
                match req.headers.get(header_name).map(|v| v.to_str()) {
                    Some(Ok(header_value)) if header_value == secret_value => true,
                    Some(Err(e)) => {
                        warn!("Weird bytes in header {header_name}: {e}");
                        false
                    }
                    _ => false,
                }
            }
        }
    }

    /// Requires a header for every endpoint under this path.  Longer paths override existing prefixes.  Error if
    /// this same path has already been added.
    fn add(&mut self, path: &str, header: RequiredHeader) -> Result<()> {
        if self.paths.insert(path.into(), header).is_some() {
            anyhow::bail!("Repeated path in header authorization: {path}");
        }
        Ok(())
    }
}

/// Describes a header that a request must include.
#[derive(Clone, Default, Debug)]
struct RequiredHeader {
    header_name: String,
    /// Names a secret (see secrets.rs) whose value must match the header value.
    secret_name: String,
    /// HTTP methods to which this requirement applies.  If absent, apply to all methods.
    methods: Option<Vec<hyper::Method>>,
}

#[derive(Clone, Default)]
pub struct PolicySystem {
    /// Maps labels to their applicable policies.
    pub labels: HashMap<String, Policy>,
    pub user_authorization: UserAuthorization,
    pub secret_authorization: SecretAuthorization,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
struct MandatoryHeader {
    name: String,
    secret_value_ref: String,
    only_for_methods: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
struct Route {
    path: String,
    users: Option<String>,
    mandatory_header: Option<MandatoryHeader>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
struct Label {
    name: String,
    transform: Option<String>,
    except_uri: Option<String>,
}

type Routes = Vec<Route>;
type Endpoints = Vec<Route>;
type Labels = Vec<Label>;

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
struct YamlPolicies {
    routes: Option<Routes>,
    endpoints: Option<Endpoints>,
    labels: Option<Labels>,
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
        let parsed_yaml: YamlPolicies = serde_yaml::from_str(config)?;
        for label in parsed_yaml.labels.unwrap_or_default() {
            let except_uri = regex::Regex::new(&label.except_uri.unwrap_or("^$".into()))?;
            match label.transform {
                Some(s) if s == "anonymize" => {
                    policies.labels.insert(
                        label.name,
                        Policy {
                            kind: Kind::Transform(crate::policies::anonymize),
                            except_uri,
                        },
                    );
                }
                Some(s) if s == "omit" => {
                    policies.labels.insert(
                        label.name,
                        Policy {
                            kind: Kind::Omit,
                            except_uri,
                        },
                    );
                }
                Some(s) if s == "match_login" => {
                    policies.labels.insert(
                        label.name,
                        Policy {
                            kind: Kind::MatchLogin,
                            except_uri,
                        },
                    );
                }
                Some(x) => {
                    anyhow::bail!("unknown transform: {x} for label {}", label.name);
                }
                None => {}
            };
        }

        let routes = parsed_yaml
            .routes
            .or(parsed_yaml.endpoints)
            .unwrap_or_default();

        for route in routes {
            if let Some(users) = route.users {
                policies
                    .user_authorization
                    .add(&route.path, regex::Regex::new(&users)?)?;
            }
            if let Some(header) = route.mandatory_header {
                let methods = parse_methods(header.only_for_methods)?;
                policies.secret_authorization.add(
                    &route.path,
                    RequiredHeader {
                        header_name: header.name,
                        secret_name: header.secret_value_ref,
                        methods,
                    },
                )?;
            }
        }
        Ok(policies)
    }
}

/// Parses v's elements into Methods.  Returns Err if an element failed to parse.
fn parse_methods(v: Option<Vec<String>>) -> Result<Option<Vec<hyper::Method>>> {
    match v {
        None => Ok(None),
        Some(v) => {
            let mut methods = vec![];
            for s in v.iter() {
                use anyhow::Context;
                use std::str::FromStr;
                methods.push(
                    hyper::Method::from_str(s)
                        .with_context(|| format!("Error parsing method {s}"))?,
                );
            }
            Ok(Some(methods))
        }
    }
}

pub fn anonymize(_: EntityValue) -> EntityValue {
    // TODO: use type-specific anonymization.
    EntityValue::String("xxxxx".to_owned())
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EntityPolicy {
    policies: chiselc::policies::Policies,
}

impl EntityPolicy {
    #[allow(dead_code)]
    pub fn from_policy_code(code: String) -> Result<Self> {
        let ctx = ParserContext::new();
        let module = ctx.parse(code, true)?;
        let policies = chiselc::policies::Policies::parse(&module);
        Ok(Self { policies })
    }
}
