// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;

#[chisel_macros::test(modules = Node)]
pub async fn test_kafka_apply(c: TestContext) {
    c.chisel.write(
        "events/test-topic.ts",
        r##"
        import { ChiselEvent } from "@chiselstrike/api";

        export default async function (event: ChiselEvent) {
            console.log(event);
        }
    "##,
    );

    c.chisel.apply().await.expect("Event handler defined: /dev/test-topic");
}
