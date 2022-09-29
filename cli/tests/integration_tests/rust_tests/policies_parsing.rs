// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;

#[chisel_macros::test(modules = Node)]
pub async fn top_level_unknown_key(c: TestContext) {
    c.chisel
        .write("policies/p.yaml", "neither_labels_nor_routes: 0");
    c.chisel
        .apply_err()
        .await
        .stderr
        .read("neither_labels_nor_routes");

    c.chisel.write("policies/p.yaml", "43289204: 0");
    c.chisel.apply_err().await.stderr.read("43289204");
}

#[chisel_macros::test(modules = Node)]
pub async fn top_level_number(c: TestContext) {
    c.chisel.write("policies/p.yaml", "0");
    c.chisel
        .apply_err()
        .await
        .stderr
        .read("invalid type: integer");
}

#[chisel_macros::test(modules = Node)]
pub async fn top_level_string(c: TestContext) {
    c.chisel.write("policies/p.yaml", "abc");
    c.chisel
        .apply_err()
        .await
        .stderr
        .read("invalid type: string \"abc\"");
}

#[chisel_macros::test(modules = Node)]
pub async fn labels_nonarray(c: TestContext) {
    c.chisel.write("policies/p.yaml", "labels: {}");
    c.chisel.apply_err().await.stderr.read("invalid type: map");
}

#[chisel_macros::test(modules = Node)]
pub async fn label_nameless(c: TestContext) {
    c.chisel
        .write("policies/p.yaml", "labels: [{ transform: omit }]");
    c.chisel
        .apply_err()
        .await
        .stderr
        .read("missing field `name`");
}

#[chisel_macros::test(modules = Node)]
pub async fn label_invalid_name(c: TestContext) {
    c.chisel.write("policies/p.yaml", "labels: [{ name: {} }]");
    c.chisel.apply_err().await.stderr.read("name: invalid type");
}

#[chisel_macros::test(modules = Node)]
pub async fn label_invalid_excepturi(c: TestContext) {
    c.chisel.write(
        "policies/p.yaml",
        "labels: [{ name: a, except_uri: [a, b] }]",
    );
    c.chisel
        .apply_err()
        .await
        .stderr
        .read("except_uri: invalid type");
}

#[chisel_macros::test(modules = Node)]
pub async fn label_not_dict(c: TestContext) {
    c.chisel.write("policies/p.yaml", "labels: [abc]");
    c.chisel
        .apply_err()
        .await
        .stderr
        .read("labels[0]: invalid type");
}

#[chisel_macros::test(modules = Node)]
pub async fn label_unknown_key(c: TestContext) {
    c.chisel
        .write("policies/p.yaml", "labels: [{ name: a, randomxyz: 0 }]");
    c.chisel.apply_err().await.stderr.read("randomxyz");

    c.chisel
        .write("policies/p.yaml", "labels: [{ name: a, 84390232: 0 }]");
    c.chisel.apply_err().await.stderr.read("84390232");
}

#[chisel_macros::test(modules = Node)]
pub async fn label_unknown_transform(c: TestContext) {
    c.chisel.write(
        "policies/p.yaml",
        "labels: [{ name: a, transform: rrraaannnddd }]",
    );
    c.chisel.apply_err().await.stderr.read("rrraaannnddd");

    c.chisel.write(
        "policies/p.yaml",
        "labels: [{ name: a, transform: 309842 }]",
    );
    c.chisel.apply_err().await.stderr.read("309842");
}

#[chisel_macros::test(modules = Node)]
pub async fn routes_nonarray(c: TestContext) {
    c.chisel.write("policies/p.yaml", "routes: {}");
    c.chisel
        .apply_err()
        .await
        .stderr
        .read("routes: invalid type");
}

#[chisel_macros::test(modules = Node)]
pub async fn route_pathless(c: TestContext) {
    c.chisel.write("policies/p.yaml", "routes: [{}]");
    c.chisel
        .apply_err()
        .await
        .stderr
        .read("missing field `path`");
}

#[chisel_macros::test(modules = Node)]
pub async fn route_invalid_path(c: TestContext) {
    c.chisel.write("policies/p.yaml", "routes: [{ path: [] }]");
    c.chisel.apply_err().await.stderr.read("path: invalid type");
}

// TODO: enable when serde_yaml rejects non-strings
// #[chisel_macros::test(modules = Node)]
// pub async fn _route_invalid_users(c: TestContext) {
//     c.chisel
//         .write("policies/p.yaml", "routes: [{ path: /, users: false }]");
//     c.chisel
//         .apply_err()
//         .await
//         .stderr
//         .read("route users isn't a string");
// }

#[chisel_macros::test(modules = Node)]
pub async fn route_unknown_key(c: TestContext) {
    c.chisel
        .write("policies/p.yaml", "routes: [{ path: /, randomxyz: 0 }]");
    c.chisel.apply_err().await.stderr.read("randomxyz");

    c.chisel
        .write("policies/p.yaml", "routes: [{ path: /, 84390232: 0 }]");
    c.chisel.apply_err().await.stderr.read("84390232");
}
