// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

async function getUser(token) {
    if (!token) return null;
    const resp = await fetch(`http://localhost:8080/__chiselstrike/auth/user/${token}`);
    return resp.ok ? await resp.text() : "failure: " + resp.status;
}

export async function getChiselStrikeClient(session, urlParameters) {
    if (session.chiselstrikeClient && session.chiselstrikeClient.token) return session.chiselstrikeClient;
    const token = urlParameters.chiselstrike_token ?? null;
    session.chiselstrikeClient = {
        user: await getUser(token),
        token,
        loginLink: `https://github.com/login/oauth/authorize?scope=read:user&client_id=2d4672c296ae275cd320`,
        backend_url: 'http://localhost:8080/',
    };
    await session.save();
    return session.chiselstrikeClient;
}

export async function chiselFetch(client, endpoint, opts) {
    if (client.token) {
        if (!opts.headers) opts.headers = {}
        opts.headers.ChiselStrikeToken = client.token;
    }
    return await fetch(client.backend_url + endpoint, opts)
}
