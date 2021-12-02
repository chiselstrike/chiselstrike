export async function getChiselStrikeClient(session, urlParameters) {
    if (session.chiselstrikeClient && session.chiselstrikeClient.isLoggedIn) return session.chiselstrikeClient;
    const token = urlParameters.chiselstrike_token;
    session.chiselstrikeClient = {
        user: token ? '[username here]' : null,
        isLoggedIn: !!token,
        loginLink: `https://github.com/login/oauth/authorize?scope=read:user&client_id=${process.env.TAOID}`,
    };
    await session.save();
    return session.chiselstrikeClient;
}
