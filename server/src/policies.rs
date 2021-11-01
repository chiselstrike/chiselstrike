// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use serde_json::{json, Value};
use std::collections::HashMap;

/// Maps labels to their applicable policies.
pub type LabelPolicies = HashMap<String, fn(Value) -> Value>;

/// Maps a field name to the transformation we apply to that field's values.
pub type FieldPolicies = HashMap<String, fn(Value) -> Value>;

pub fn anonymize(_: Value) -> Value {
    // TODO: use type-specific anonymization.
    json!("xxxxx")
}
