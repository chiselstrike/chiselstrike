use crate::worker::WorkerState;
use std::collections::HashMap;

// Overrides a Deno op, which requires a filesystem read permissions, with a dummy implementation.
// This is needed because some Node libraries (such as `memfs`) use `process.cwd` without
// restraint, even if they don't try to read anything from the filesystem.
#[deno_core::op]
pub fn op_cwd() -> &'static str {
    "/"
}

#[deno_core::op]
pub fn op_set_env(state: &mut deno_core::OpState, key: String, value: String) {
    state
        .borrow_mut::<WorkerState>()
        .fake_env
        .insert(key, value);
}

#[deno_core::op]
pub fn op_env(state: &mut deno_core::OpState) -> HashMap<String, String> {
    state.borrow::<WorkerState>().fake_env.clone()
}

#[deno_core::op]
pub fn op_get_env(state: &mut deno_core::OpState, key: String) -> Option<String> {
    state.borrow::<WorkerState>().fake_env.get(&key).cloned()
}

#[deno_core::op]
pub fn op_delete_env(state: &mut deno_core::OpState, key: String) {
    state.borrow_mut::<WorkerState>().fake_env.remove(&key);
}
