// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

// @deno-types="./deno_types_imp.d.ts"
import { foo } from "./deno_types_imp.js";
function bar(): string {
    return foo();
}
