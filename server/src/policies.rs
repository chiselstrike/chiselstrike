// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use serde_json::{json, Value};
use std::collections::HashMap;

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

pub(crate) fn anonymize(_: Value) -> Value {
    // TODO: use type-specific anonymization.
    json!("xxxxx")
}
