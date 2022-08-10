// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

// This is code for the special `__chiselstrike` version in chiseld

import { ChiselEntity } from './datastore.ts';
import { ChiselRequest } from './request.ts';
import { RouteMap, MiddlewareNext } from './routing.ts';
import { getSecret } from './utils.ts';

class AuthUser extends ChiselEntity {}
class AuthSession extends ChiselEntity {}
class AuthToken extends ChiselEntity {}
class AuthAccount extends ChiselEntity {}

export default new RouteMap()
    .prefix('/auth', new RouteMap()
        .prefix('/users', AuthUser.crud())
        .prefix('/sessions', AuthSession.crud())
        .prefix('/tokens', AuthToken.crud())
        .prefix('/accounts', AuthAccount.crud())
        .middleware(authMiddleware)
    );

async function authMiddleware(request: ChiselRequest, next: MiddlewareNext): Promise<Response> {
    const authHeader = request.headers.get('ChiselAuth');
    if (authHeader === null) {
        // TODO: use a better error message
        return forbidden('AuthSecret');
    }

    const expectedSecret = getSecret('CHISELD_AUTH_SECRET');
    if (expectedSecret === undefined) {
        // TODO: use a better error message
        return forbidden('ChiselAuth');
    }

    if (authHeader !== expectedSecret) {
        // TODO: use a better error message
        return forbidden('Fundamental auth');
    }

    return next(request);
}

function forbidden(msg: string): Response {
    return new Response(msg, { status: 403 });
}

