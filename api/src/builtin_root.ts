// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

// This is code for the special `__chiselstrike` version in chiseld

import { ChiselEntity } from "./datastore.ts";
import { TopicMap } from "./kafka.ts";
import { ChiselRequest } from "./request.ts";
import { MiddlewareNext, RouteMap } from "./routing.ts";
import { getSecret, opSync } from "./utils.ts";

class AuthUser extends ChiselEntity {}
class AuthSession extends ChiselEntity {}
class AuthToken extends ChiselEntity {}
class AuthAccount extends ChiselEntity {}

const isDebug = opSync("op_chisel_is_debug") as boolean;

export const routeMap = new RouteMap()
    .prefix(
        "/auth",
        new RouteMap()
            .prefix("/users", RouteMap.convert(AuthUser.crud()))
            .prefix("/sessions", RouteMap.convert(AuthSession.crud()))
            .prefix("/tokens", RouteMap.convert(AuthToken.crud()))
            .prefix("/accounts", RouteMap.convert(AuthAccount.crud()))
            .middleware(authMiddleware),
    );

export const topicMap = new TopicMap();

// deno-lint-ignore require-await
async function authMiddleware(
    request: ChiselRequest,
    next: MiddlewareNext,
): Promise<Response> {
    const expectedSecret = getSecret("CHISELD_AUTH_SECRET");
    if (expectedSecret === undefined && isDebug) {
        return forbidden(
            "To access this route, please configure the secret `CHISELD_AUTH_SECRET` " +
                "and then pass its value in the `ChiselAuth` header",
        );
    }

    const authHeader = request.headers.get("ChiselAuth");
    if (authHeader === null) {
        return forbidden("Please use the `ChiselAuth` header");
    }

    if (expectedSecret === undefined || authHeader !== expectedSecret) {
        // TODO: use a better error message
        return forbidden("Incorrect value of the `ChiselAuth` header");
    }

    return next(request);
}

function forbidden(msg: string): Response {
    return new Response(msg, { status: 403 });
}
