import { runBasicTests } from "@next-auth/adapter-test";
import { ChiselAdapter } from "../lib";
import "isomorphic-fetch";

const adapter = ChiselAdapter({ url: "http://localhost:8080", secret: "1234" });

runBasicTests({
    adapter,
    db: {
        async user(id) {
            return await adapter.getUser(id);
        },
        async session(token) {
            return await adapter.getSession(token);
        },
        async account({ provider, providerAccountId }) {
            return await adapter.getAccount(provider, providerAccountId);
        },
        async verificationToken({ token, identifier }) {
            return await adapter.getVerificationToken(token, identifier);
        },
    },
});
