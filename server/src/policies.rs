// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::prefix_map::PrefixMap;
use crate::types::ObjectType;
use crate::JsonObject;
use anyhow::Result;
use hyper::Request;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use yaml_rust::{Yaml, YamlLoader};

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

/// Maps labels to their applicable policies.
pub type LabelPolicies = HashMap<String, Policy>;

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
    pub fn is_allowed(&self, username: Option<String>, path: &str) -> bool {
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

/// Describes secret-based authorization.  An endpoint request will only be allowed if it includes a header
/// specified in this struct.
#[derive(Clone, Default, Debug)]
pub struct SecretAuthorization {
    /// A request can access an endpoint if it includes a header required by the longest path prefix.
    paths: PrefixMap<RequiredHeader>,
}

impl SecretAuthorization {
    /// Is a request with these headers allowed to execute the endpoint at this path?
    pub fn is_allowed(&self, req: &Request<hyper::Body>, secrets: &JsonObject, path: &str) -> bool {
        match self.paths.longest_prefix(path) {
            None => true,
            Some((
                _,
                RequiredHeader {
                    methods: Some(v), ..
                },
            )) if !v.contains(req.method()) => true,
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
                match req.headers().get(header_name).map(|v| v.to_str()) {
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
pub struct VersionPolicy {
    pub labels: LabelPolicies,
    pub user_authorization: UserAuthorization,
    pub secret_authorization: SecretAuthorization,
}

#[derive(Clone, Default)]
pub struct Policies {
    pub versions: HashMap<String, VersionPolicy>,
}

impl Policies {
    pub fn add_from_yaml(&mut self, version: String, yaml: &str) -> Result<()> {
        let v = VersionPolicy::from_yaml(yaml)?;
        self.versions.insert(version, v);
        Ok(())
    }

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

        if let Some(version) = self.versions.get(&ty.api_version) {
            for fld in ty.user_fields() {
                for lbl in &fld.labels {
                    if let Some(p) = version.labels.get(lbl) {
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
        }
        field_policies
    }
}

impl VersionPolicy {
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

            #[allow(clippy::or_fun_call)]
            let routes = config["routes"]
                .as_vec()
                .or(config["endpoints"].as_vec())
                .map(|vec| vec.iter())
                .into_iter()
                .flatten();

            for route in routes {
                if let Some(path) = route["path"].as_str() {
                    if let Some(users) = route["users"].as_str() {
                        policies
                            .user_authorization
                            .add(path, regex::Regex::new(users)?)?;
                    }
                    let header = &route["mandatory_header"];
                    match header {
                        Yaml::BadValue => {}
                        Yaml::Hash(_) => {
                            let kv = (&header["name"], &header["secret_value_ref"]);
                            match kv {
                                (Yaml::String(name), Yaml::String(value)) => {
                                    let methods = &header["only_for_methods"];
                                    let methods = match methods {
                                        Yaml::BadValue => None,
                                        Yaml::String(_) => Some(parse_methods(&vec![methods.clone()])?),
                                        Yaml::Array(a) => Some(parse_methods(a)?),
                                        _ => {
                                            warn!("only_for_methods must be a list of strings, instead got {methods:?}");
                                            None
                                        }
                                    };
                                    policies.secret_authorization.add(path, RequiredHeader {
                                        header_name: name.clone(),
                                        secret_name: value.clone(),
                                        methods,
                                    })?;
                                }
                                _ => anyhow::bail!(
                                    "Header must have string values for keys 'name' and 'secret_value_ref'. Instead got: {header:?}"
                                ),
                            }
                        }
                        x => anyhow::bail!("Unparsable header: {x:?}"),
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

fn parse_methods(v: &Vec<Yaml>) -> Result<Vec<hyper::Method>> {
    let mut methods = vec![];
    for e in v {
        use anyhow::Context;
        use std::str::FromStr;
        match e {
            Yaml::String(s) => methods.push(
                hyper::Method::from_str(s).with_context(|| format!("Error parsing method {s}"))?,
            ),
            _ => anyhow::bail!("String method expected in only_for_methods, instead got {e:?}"),
        }
    }
    Ok(methods)
}
