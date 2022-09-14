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
    c.chisel.apply_err().await.stderr.read("isn't a dictionary");
}

#[chisel_macros::test(modules = Node)]
pub async fn top_level_string(c: TestContext) {
    c.chisel.write("policies/p.yaml", "abc");
    c.chisel.apply_err().await.stderr.read("isn't a dictionary");
}

#[chisel_macros::test(modules = Node)]
pub async fn labels_nonarray(c: TestContext) {
    c.chisel.write("policies/p.yaml", "labels: {}");
    c.chisel.apply_err().await.stderr.read("value for labels");
}

#[chisel_macros::test(modules = Node)]
pub async fn routes_nonarray(c: TestContext) {
    c.chisel.write("policies/p.yaml", "routes: {}");
    c.chisel.apply_err().await.stderr.read("value for routes");
}
