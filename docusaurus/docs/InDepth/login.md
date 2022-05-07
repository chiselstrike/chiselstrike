# User Login

ChiselStrike supports the process of having your own users.  Your
website can let them log in (eg, using an OAuth provider), and your
ChiselStrike backend will be aware of who logged in and even maintain
a collection of all known users.  Let's examine how this works in
ChiselStrike.

An easy way to support many OAuth providers, as well as email and SMS
authentication is to leverage the [NextAuth.js
framework](https://next-auth.js.org/) in the frontend.  NextAuth is
popular, featureful, and open-source.  It takes little effort to set
up and can be used with both NextJS and
[Gatsby](https://github.com/nextauthjs/next-auth-gatsby-example).

Rather than roll our own OAuth implementations, we decided to rely on
NextAuth and what it readily provides.  We have developed a
ChiselStrike adapter for NextAuth; when configured to use this
adapter, NextAuth will save the user data in your ChiselStrike
backend, where your endpoints and policies can use it.

As suggested in the adapter README, your frontend can tell
ChiselStrike who's currently logged in by including a `ChiselUID`
header in your requests.  The value of this header should be the
NextAuth user ID.

## Accessing User Info in the Backend

The ChiselStrike backend keeps track of your website's users via a
builtin type called AuthUser.  When a user logs in for the first
time, a new AuthUser entity is added.  And when an endpoint is
executed, it has access to this builtin type.

One interesting thing to do is have AuthUser-typed fields in your
entities.  For example:

```typescript title="my-backend/models/models.ts"
import { AuthUser, ChiselEntity } from "@chiselstrike/api"

export class BlogComment extends ChiselEntity {
    content: string = "";
    author: AuthUser;
}
```

This makes it easy to link a `BlogComment` to the user who created it:
you simply provide a special `author` value when creating it.  The
ChiselStrike API includes a function named `loggedInUser`, which
returns the AuthUser object corresponding to the user currently
logged in (or `undefined` if no one is logged in).  This works, for
example:

```typescript title="my-backend/endpoints/example.ts"
import { BlogComment } from '../models/models.ts';
import { loggedInUser, responseFromJson } from '@chiselstrike/api';
export default async function (req) {
    let c = BlogComment.build(await req.json());
    c.author = await loggedInUser();
    if (c.author === undefined) { return responseFromJson('Must be logged in', 401) }
    await c.save();
    return responseFromJson('saved successfully');
}
```

You can even restrict a user's access to only their own comments;
please see ["Restricting Data Access to Matching
User"](pol#restricting-data-access-to-matching-user).
