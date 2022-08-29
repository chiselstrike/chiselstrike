# Data Policies

ChiselStrike lets you express data policies through a policy file that
succinctly expresses your rules for how the data is served from
storage.  Although the policy language is currently limited, it is set
to rapidly expand in the immediate future.

Examples used in this chapter build on the `my-backend` files from the
[Introduction](Intro/first.md).

## Data Transformation

The most basic policy you could enact is saying that the data read
from ChiselStrike shall be systematically transformed before it's sent
to the frontend.  You basically say "this kind of data must be
transformed like this whenever it is accessed".

Let's first examine more precisely how you define which data we're
talking about.  This is done using _labels_: TypeScript decorators on
the fields of your models.

For instance, suppose we have a BlogComment object defined and add the "labels"
decorator as shown below:

```typescript title="my-backend/models/models.ts"
import { ChiselEntity, labels } from "@chiselstrike/api"

export class BlogComment extends ChiselEntity {
    content: string;
    @labels("pii") by: string;
}
```

Labels are specified by using the `@labels` decorator with a list of strings. Each
individual string denotes a label.

We add the `pii` label to the `by` field, because we intend to
refer to it when dictating how `by` should be treated.

:::note
You can pick any name for a label.  We don't have any restrictions or
conventions at this time.
:::

Now let's enforce a transformation on `pii` fields.  Please create
the file `my-backend/policies/pol.yml` like this:

```yaml title="my-backend/policies/pol.yml"
labels:
  - name: pii
    transform: anonymize
```

When you save this file, you should see this in the `chisel dev`
output:

```
Policy defined for label pii
```

And now notice how the output of the `comments` endpoint changes.

If send a `GET /dev/comments` request with:

```bash
curl -s localhost:8080/dev/comments
```

The `curl` command reports:

```json
{
    "results": [
        {
            "id": "a4ca3ab3-2e26-4da6-a5de-418c1e6b9b83",
            "content": "First comment",
            "by": "xxxxx"
        },
        {
            "id": "fed312d7-b36b-4f34-bb04-fba327a3f440",
            "content": "Second comment",
            "by": "xxxxx"
        },
        {
            "id": "adc89862-dfaa-43ab-a639-477111afc55e",
            "content": "Third comment",
            "by": "xxxxx"
        },
        {
            "id": "5bfef47e-371b-44e8-a2dd-88260b5c3f2c",
            "content": "Fourth comment",
            "by": "xxxxx"
        }
    ]
}
```

The `pii` fields were anonymized!  It is not possible for any
request handler code to accidentally read `pii` data, eliminating human
errors from the process.

Another transformation you can do is omitting a field altogether.  If you modify
the file `my-backend/policies/pol.yml` this way:

```yaml title="my-backend/policies/pol.yml"
labels:
  - name: pii
    transform: omit
```

your code will not see the existence of any `@pii` fields:

```bash
curl -s localhost:8080/dev/comments
```

will return

```json
{
    "results": [
        {
            "id": "a4ca3ab3-2e26-4da6-a5de-418c1e6b9b83",
            "content": "First comment",
        },
        {
            "id": "fed312d7-b36b-4f34-bb04-fba327a3f440",
            "content": "Second comment",
        },
        {
            "id": "adc89862-dfaa-43ab-a639-477111afc55e",
            "content": "Third comment",
        },
        {
            "id": "5bfef47e-371b-44e8-a2dd-88260b5c3f2c",
            "content": "Fourth comment",
        }
    ]
}
```

## Policy Exceptions

Here is how you can except request handlers under the `comments` path from automatic
data anonymization.  Please edit the file
`my-backend/policies/pol.yml` like this:

```yaml title="my-backend/policies/pol.yml"
labels:
  - name: pii
    transform: anonymize
    except_uri: /comments
```

The `except_uri` key lets you specify a path that's exempt from the
transformation policy being defined.  In this case, it matches exactly
the `comments` request handler.  But in general, the value can be a path
prefix and even a regular expression; any matching requests will be
exempt from the policy.

If you now send the `GET /dev/comments` request:

```bash
curl -s localhost:8080/dev/comments
```

The `curl` command reports:

```json
{
    "results": [
        {
            "id": "a4ca3ab3-2e26-4da6-a5de-418c1e6b9b83",
            "content": "First comment",
            "by": "Jill"
        },
        {
            "id": "fed312d7-b36b-4f34-bb04-fba327a3f440",
            "content": "Second comment",
            "by": "Jack"
        },
        {
            "id": "adc89862-dfaa-43ab-a639-477111afc55e",
            "content": "Third comment",
            "by": "Jim"
        },
        {
            "id": "5bfef47e-371b-44e8-a2dd-88260b5c3f2c",
            "content": "Fourth comment",
            "by": "Jack"
        }
    ]
}
```

As you can see, this request handler now operates with the raw, untransformed
data.

## Restricting Access to Routes

You may or may not want to allow everyone on the Internet to invoke your
ChiselStrike endpoints.  In case you don't, we got you covered via our policy
capabilities.  You can restrict access to individual endpoints or entire
subdirectories.  And you can treat different HTTP methods differently.

Route access can be restricted in two different ways: a _shared secret_ or a
_proof of user login_.  A shared secret is a value that a request must include
to be authorized.  It is typically long-lived and known to all valid clients.  A
proof of user login, on the other hand, is user-specific and typically
short-lived.  It is obtained when a user logs in successfully in the client.
The shared-secret method is considerably simpler to set up, but the user-login
one offers additional capabilities, such as different permissions for different
users and preventing one user from accessing another user's data.

When restricting routes, you use a top-level entry named `routes` in the policy
YAML.  It is a list of route-specific policies, each of which begins with a
`path` entry that denotes the route to which the restriction applies.

For example, this YAML:

```yaml title="my-backend/policies/pol.yml"
routes:
  - path: /comments
    # corresponding restriction goes here
  - path: /
    # corresponding restriction goes here
  - path: /users
    # corresponding restriction goes here
```

defines three restrictions: one for route `/comments`, one for route `/`, and
one for route `/users`.  As you can see, `path` values may overlap, in which
case longer overrides shorter.  When a request arrives, the longest prefix of
its path that matches some `path` entry will dictate which restriction applies.
Although multiple paths may overlap, they must not be identical.

### Shared Secret

To restrict a route via a shared secret, you set a policy that names that route
and specifies how clients will provide the secret value.  Then a client's
request must conform to this specification in order to be allowed to proceed.
In particular, the request must provide the secret value via a certain HTTP
header described by the policy.  Consider this example policy:

```yaml title="my-backend/policies/pol.yml"
routes:
  - path: /comments
    mandatory_header: { name: header123, secret_value_ref: TOKEN123 }
```

This says that any request accessing the `/comments` route must provide an HTTP
header named `header123` with a value equaling that of the secret TOKEN123.
(See the ["Secrets"](./secrets.md) section to learn how to set secret values in
ChiselStrike.  The secret value in this case must be a string.)  The
`mandatory_header` entry is a dictionary containing at least the `name` and
`secret_value_ref` keys, which together describe the HTTP header required for a
request to succeed.  Requests without this header or with a wrong value will be
rejected with status 403 Forbidden.

#### Exempting Some HTTP Methods

If you want to restrict a route only for some HTTP methods (eg, anyone can GET,
but only a restricted few can PUT), use an `only_for_methods` entry in
`mandatory_header`.  For example:

```yaml title="my-backend/policies/pol.yml"
routes:
  - path: /comments
    mandatory_header: { name: header123, secret_value_ref: TOKEN123, only_for_methods: [ PUT, POST, PATCH ] }
```

The value of `only_for_methods` is a list of methods covered by this
restriction.  Any method not on the list will be unrestricted.

### User Login

ChiselStrike supports [having users log into your dynamic
website](./login.md).  It even lets you restrict access to HTTP routes by
user.

To restrict who can access a path, provide a `users` entry with a regular
expression describing the permitted users.  For example:

```yaml title="my-backend/policies/pol.yml"
routes:
  - path: /comments
    users: ^admin@example.com$
```

This says that only the user `admin@example.com` can send requests to `/comments`.

:::note
We currently match `users` against the user's email -- the only field
NextAuth guarantees to be unique.  Your feedback is welcome as we
evolve this aspect of our product.
:::

When `users` is specified, anonymous access to the path will be
prohibited.  For example, if you want to force the user to log in to
access `comments` but don't care which specific user is accessing it,
you can set `users` to `.*`.

#### Restricting Data Access to Matching User

As explained in ["Accessing User Info in the
Backend"](login#accessing-user-info-in-the-backend), you can store the
logged-in user as a field in your entities.  Let's continue the
example from that link here.  Please edit the file `models/models.ts`
like this:

```typescript title="my-backend/models/models.ts"
import { ChiselEntity } from "@chiselstrike/api"

export class BlogComment extends ChiselEntity {
    content: string = "";
    @labels("protect") author: AuthUser;
}
```

Then please add the following policy:

```yaml title="my-backend/policies/pol.yml"
labels:
  - name: protect
    transform: match_login
```

The `match_login` transformation compares fields labeled with
`protect` (if they are of AuthUser type) to the value of
`loggedInUser()`.  When the field value doesn't match, the row is
ignored.  So in this case, when request handlers read BlogComment entities,
they will see only the rows whose `author` matches the currently
logged-in user.

:::tip
You can use `except_uri` here, and it works the same as described
above.
:::
