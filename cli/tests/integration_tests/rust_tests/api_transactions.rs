// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;

#[chisel_macros::test(modules = Deno)]
pub async fn missing_await(c: TestContext) {
    c.chisel.write(
        "models/user.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";
        export class User extends ChiselEntity {
            username: string;
        }
    "##,
    );
    c.chisel.write(
        "routes/find.ts",
        r##"
        import { User } from "../models/user.ts";

        export default async function (req: Request): Promise<string> {
            const users = User.findAll();
            return "Hello";
        }
    "##,
    );
    c.chisel.apply_ok().await;

    let response = c.chisel.get("/dev/find").send().await;
    // Endpoint with a missing `await` can either succeed, if Deno executes
    // the call immediately, or fail, if Deno defers execution.
    if response.status() == 500 {
        response.assert_text_contains("Cannot commit a transaction because there is an operation in progress that uses this transaction");
    } else {
        response.assert_status(200).assert_text_contains("Hello");
    }
}
