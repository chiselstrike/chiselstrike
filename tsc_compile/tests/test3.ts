// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

/// <reference lib="deno.core" />

import {foo} from "./test2.ts"

function bar(): string {
    return foo();
}
