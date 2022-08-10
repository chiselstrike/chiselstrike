use crate::framework::{IntegrationTest, OpMode};

pub mod test_bad_filter;
pub mod test_find_by;
pub mod test_http_import;

use test_bad_filter::*;
use test_find_by::*;
use test_http_import::*;

macro_rules! test {
    ($func_name:ident, $mode:expr) => {
        IntegrationTest {
            name: stringify!($func_name),
            mode: $mode,
            test_fn: &$func_name,
        }
    };
}

macro_rules! deno_test {
    ($func_name:ident) => {
        test!($func_name, OpMode::Deno)
    };
}

macro_rules! node_test {
    ($func_name:ident) => {
        test!($func_name, OpMode::Node)
    };
}

pub fn all_tests() -> Vec<IntegrationTest> {
    vec![
        deno_test!(test_bad_filter),
        node_test!(test_find_by),
        node_test!(test_http_import),
    ]
}
