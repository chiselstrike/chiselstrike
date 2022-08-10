use crate::framework::{IntegrationTest, OpMode};

pub mod test_bad_filter;
pub mod test_find_by;
pub mod test_http_import;

pub fn all_tests() -> Vec<IntegrationTest> {
    vec![
        IntegrationTest {
            name: "test_bad_filter",
            mode: OpMode::Deno,
            test_fn: &test_bad_filter::test_bad_filter,
        },
        IntegrationTest {
            name: "test_find_by",
            mode: OpMode::Deno,
            test_fn: &test_find_by::test_find_by,
        },
        IntegrationTest {
            name: "test_http_import",
            mode: OpMode::Node,
            test_fn: &test_http_import::test_http_import,
        },
    ]
}
