// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

// This is the main module executed in a JavaScript runtime in `chiseld`.

// Import the user-defined code from a special module prepared by `chisel
// apply`. This transitively loads all user code.
import { routeMap, topicMap } from "file:///__root.ts";

// Continue in TypeScript.
import run from "chisel://api/run.ts";
await run(routeMap, topicMap);
