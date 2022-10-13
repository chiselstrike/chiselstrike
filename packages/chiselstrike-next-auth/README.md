# ChiselStrike Adapter for NextAuth.js

## Getting Started:

To use the adapter in your application, do the following:

```typescript
import GitHubProvider from "next-auth/providers/github";
import NextAuth from "next-auth";
import { ChiselAdapter } from "@chiselstrike/next-auth";

export const authOptions = {
    adapter: ChiselAdapter({ url: "http://localhost:8080", secret: "1234" }),
    providers: [
        GitHubProvider({
            clientId: process.env.GITHUB_ID,
            clientSecret: process.env.GITHUB_SECRET,
        }),
    ],
};
export default NextAuth(authOptions);
```

For local development, define the auth secret in `.env` file of ChiselStrike
project:

```
{ "CHISELD_AUTH_SECRET" : "1234" }
```

## Developing

To test a locally developed version, run the following commands in this source
tree:

```
npm run build && npm link
```

and the following command to use the library in your project:

```
npm link @chiselstrike/next-auth
```

Now you have the locally developed version of the adapter available for your
app.
