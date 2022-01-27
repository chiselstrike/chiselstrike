---
sidebar_position: 6
---
# Secrets

ChiselStrike supports adding and hot-reloading secrets that your application
can access at runtime.

In local development mode, these secrets are stored in plain text for your convenience
in a local file. In production, they are encrypted and safely stored.

Secrets in ChiselStrike are just a json file. Each key represents a secret that can
then be accessed by the `getSecret` function.

To see this working, let's add an `.env` file with the following contents to your working directory

```json
{
  "secret1": "mysecret",
  "secret2": {
    "key": "value",
    "otherkey": "othervalue"
  }
}
```

Now those secrets are available as objects from your typescript code

```typescript title="my-backend/endpoints/secrets.ts"
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

We can now ask for one of our secrets

```console
curl localhost:8080/dev/secrets?secret=secret1
```

and receive:

```console
"mysecret"
```

or another one of our secrets, that is a JSON object instead of a string:

```console
curl localhost:8080/dev/secrets?secret=secret2
```

and receive it back:

```console
{"key":"value","otherkey":"othervalue"}
```

:::caution
We know you know this, but a reminder is always welcome!
Never commit your secrets file to git!
:::
