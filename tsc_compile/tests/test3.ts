/// <reference lib="deno.core" />

import {foo} from "./test2.ts"

function bar(): string {
    return foo();
}
