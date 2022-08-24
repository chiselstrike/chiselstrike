# Secrets

ChiselStrike supports adding and hot-reloading secrets that your application
can access at runtime.

In local development mode, these secrets are stored in plain text for your convenience
in a local file. In production, they are encrypted and safely stored. As such, you should not
commit your local test files to version control if they contain confidential information, and should
probably add your secret file to ".gitignore" to make sure you don't.

Secrets in ChiselStrike are JSON data. Each key represents a secret that can
then be accessed by the `getSecret` function.

These keys are actually general purpose environment variables, and do not have to pertain
to anything confidential. For instance, you could use these to implement feature flags!

To see this working, let's add an `.env` file with the following contents to your working directory.
This must be explicitly named ".env", it's not a file with a ".env" suffix.

```json title=".env"
{
  "secret1": "mysecret",
  "secret2": {
    "key": "value",
    "otherkey": "othervalue"
  }
}
```

Now those values are available as objects from your typescript code:

```typescript title="my-backend/routes/secrets.ts"
import { getSecret, responseFromJson } from "@chiselstrike/api"

export default async function (req) {
    const url = new URL(req.url);
    const arg = url.searchParams.get("secret");
    if (!arg) {
        return new Response("ask for a secret");
    } else{
        const secret = getSecret(arg) ?? {};
        return responseFromJson(secret);
    }
}
```

Of course, this is an insecure demo, as we should never make an endpoint that just offers up
our secrets. But it's great for a demo!

We can now ask for one of our secrets

```console
curl localhost:8080/dev/secrets?secret=secret1
```

and receive:

```console
"mysecret"
```

or fetch another one of our secrets, that is a JSON object instead of a string:

```console
curl localhost:8080/dev/secrets?secret=secret2
```

and receive it back:

```console
{"key":"value","otherkey":"othervalue"}
```

:::caution
We know you know this, but a reminder is always welcome!
Never commit your secrets file to git, and don't expose them where users
can ask for them!
:::


