// SPDX-FileCopyrightText: © 2023 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;

#[chisel_macros::test(modules = Node)]
pub async fn find_many(c: TestContext) {
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap, JsonRequest } from '@chiselstrike/api';

        type Query = {
            count: number;
        };
        type Body = {
            name: string;
        }
        export default new RouteMap()
            .post('/foo', async (req: JsonRequest<Query, Body>) => {
                const q = req.queryParams();
                const body = await req.jsonBody();
                const count: number = q.count;
                const name: string = body.name;

                return [count, name];
            });
        "#,
    );

    c.chisel.apply_ok().await;
    c.chisel
        .post("/dev/foo?count=42")
        .json(json!({"name": "Jan"}))
        .send()
        .await
        .assert_json(json!([42, "Jan"]));

    c.chisel
        .post("/dev/foo?count=foo")
        .json(json!({"name": "Jan"}))
        .send()
        .await
        .assert_status(400)
        .text();
}
