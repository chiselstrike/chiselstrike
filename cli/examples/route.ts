// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
import { RouteMap } from "@chiselstrike/api";

// The request handler is a function that evaluates to a promise
// that resolves to a Response.

async function handleGet(req: Request): Promise<Response> {
    return new Response(req.body, {
        status: 203,
        headers: [
            ["foo", "bar"],
            ["baz", "zed"]
        ]
    });
}

export default new RouteMap()
    .get("/", handleGet);
