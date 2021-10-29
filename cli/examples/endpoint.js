// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

// The endpoint consists of a function that evaluates to a promise
// that resolves to a Response.

export default async function chisel(req) {
    return new Response(req.body, {
        status: 203,
        headers: [
            ["foo", "bar"],
            ["baz", "zed"]
        ]
    });
}
