export function ChiselAdapter(options: { url: string; secret: string }) {
    const headers = { "ChiselAuth": options.secret };
    const url = options.url + "/__chiselstrike/auth/";
    return {
        async createUser(data) {
            const user = JSON.stringify(data);
            const response = await fetch(`${url}users`, {
                method: "POST",
                headers,
                body: user,
            });
            if (!response.ok) {
                return null;
            }
            return toUser(await response.json());
        },
        async getUser(userId) {
            return await fetchUserById(url, headers, userId);
        },
        async getUserByEmail(email) {
            return await fetchUserByEmail(url, headers, email);
        },
        async getUserByAccount({ provider, providerAccountId }) {
            const account = await fetchAccount(
                url,
                headers,
                provider,
                providerAccountId,
            );
            if (!account) {
                return null;
            }
            return await fetchUserById(url, headers, account.userId);
        },
        async updateUser(updatedUser) {
            const user = await fetchUserById(url, headers, updatedUser.id);
            if (!user) {
                return;
            }
            await fetch(`${url}users/${user.id}`, {
                method: "PATCH",
                headers,
                body: JSON.stringify(updatedUser),
            });
            return await fetchUserById(url, headers, updatedUser.id);
        },
        async deleteUser(userId) {
            await fetch(`${url}users/${userId}`, { method: "DELETE", headers });
            await fetch(`${url}sessions?.userId=${userId}`, {
                method: "DELETE",
                headers,
            });
            await fetch(`${url}accounts?.userId=${userId}`, {
                method: "DELETE",
                headers,
            });
        },
        async getAccount(provider, providerAccountId) {
            return await fetchAccount(
                url,
                headers,
                provider,
                providerAccountId,
            );
        },
        async linkAccount(data) {
            const account = JSON.stringify(data);
            const response = await fetch(`${url}accounts`, {
                method: "POST",
                headers,
                body: account,
            });
            if (!response.ok) {
                return null;
            }
            return await response.json();
        },
        async unlinkAccount({ provider, providerAccountId }) {
            await fetch(
                `${url}accounts?.provider=${provider}&.providerAccountId=${providerAccountId}`,
                {
                    method: "DELETE",
                    headers,
                },
            );
        },
        async createSession(newSession: { sessionToken; userId; expires }) {
            const body = JSON.stringify(newSession);
            const response = await fetch(`${url}sessions`, {
                method: "POST",
                headers,
                body,
            });
            if (!response.ok) {
                return null;
            }
            return toSession(await response.json());
        },
        async getSession(token) {
            return await fetchSession(url, headers, token);
        },
        async getSessionAndUser(sessionToken) {
            const session = await fetchSession(url, headers, sessionToken);
            if (!session) {
                return null;
            }
            const user = await fetchUserById(url, headers, session.userId);
            return { session, user };
        },
        async updateSession(updatedSession) {
            const session = await fetchSession(
                url,
                headers,
                updatedSession.sessionToken,
            );
            if (!session) {
                return;
            }
            await fetch(`${url}sessions/${session.id}`, {
                method: "PATCH",
                headers,
                body: JSON.stringify(updatedSession),
            });
            return await fetchSession(
                url,
                headers,
                updatedSession.sessionToken,
            );
        },
        async deleteSession(sessionToken) {
            await fetch(`${url}sessions/?.sessionToken=${sessionToken}`, {
                method: "DELETE",
                headers,
            });
        },
        async getVerificationToken(token, identifier) {
            return await fetchToken(url, headers, token, identifier);
        },
        async createVerificationToken(data: { identifier; expires; token }) {
            const token = JSON.stringify(data);
            const response = await fetch(`${url}tokens`, {
                method: "POST",
                headers,
                body: token,
            });
            if (!response.ok) {
                return null;
            }
            return toToken(await response.json());
        },
        async useVerificationToken({ identifier, token }) {
            const result = await fetchToken(url, headers, token, identifier);
            await fetch(
                `${url}tokens/?.identifier=${identifier}&.token=${token}`,
                {
                    method: "DELETE",
                    headers,
                },
            );
            return result;
        },
    };
}
async function fetchSession(url: string, headers: {}, sessionToken: string) {
    const response = await fetch(
        `${url}sessions?.sessionToken=${sessionToken}`,
        { headers },
    );
    if (!response.ok) {
        return null;
    }
    const json = await response.json();
    const sessions = json?.results;
    if (!sessions?.length) {
        return null;
    }
    return toSession(sessions[0]);
}
async function fetchUserById(url: string, headers: {}, userId: string) {
    const response = await fetch(`${url}users/${userId}`, { headers });
    if (!response.ok) {
        return null;
    }
    return toUser(await response.json());
}
async function fetchUserByEmail(url: string, headers: {}, email: string) {
    const response = await fetch(`${url}users?.email=${email}`, { headers });
    if (!response.ok) {
        return null;
    }
    const json = await response.json();
    const users = json?.results;
    if (!users?.length) {
        return null;
    }
    return toUser(users[0]);
}
async function fetchAccount(
    url: string,
    headers: {},
    provider: string,
    providerAccountId: string,
) {
    const response = await fetch(
        `${url}accounts?.provider=${provider}&.providerAccountId=${providerAccountId}`,
        { headers },
    );
    if (!response.ok) {
        return null;
    }
    const json = await response.json();
    const accounts = json?.results;
    if (!accounts?.length) {
        return null;
    }
    return accounts[0];
}
async function fetchToken(
    url: string,
    headers: {},
    token: string,
    identifier: string,
) {
    const response = await fetch(
        `${url}tokens?.token=${token}&.identifier=${identifier}`,
        { headers },
    );
    if (!response.ok) {
        return null;
    }
    const json = await response.json();
    const tokens = json?.results;
    if (!tokens?.length) {
        return null;
    }
    return toToken(tokens[0]);
}
function toUser(user) {
    return { ...user, emailVerified: new Date(user.emailVerified) };
}
function toSession(session) {
    return { ...session, expires: new Date(session.expires) };
}
function toToken(token) {
    return { ...token, expires: new Date(token.expires), id: undefined };
}
