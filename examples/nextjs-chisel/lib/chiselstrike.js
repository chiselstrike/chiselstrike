async function getUser(token) {
    if (!token) return null;
    const resp = await fetch(`http://localhost:8080/__chiselstrike/auth/user/${token}`);
    return resp.ok ? await resp.text() : "failure: " + resp.status;
}

export async function getChiselStrikeClient(session, urlParameters) {
    if (session.chiselstrikeClient && session.chiselstrikeClient.isLoggedIn) return session.chiselstrikeClient;
    const token = urlParameters.chiselstrike_token;
    session.chiselstrikeClient = {
        user: await getUser(token),
        isLoggedIn: !!token,
        loginLink: `https://github.com/login/oauth/authorize?scope=read:user&client_id=${process.env.TAOID}`,
        backend_url: 'http://localhost:8080/',
    };
    await session.save();
    return session.chiselstrikeClient;
}

export async function chiselFetch(client, endpoint, opts) {
    // FIXME: we should use client.backend_url instead of localhost:3000 here, but that currently fails CORS preflight.
    return await fetch(endpoint, opts)
}
