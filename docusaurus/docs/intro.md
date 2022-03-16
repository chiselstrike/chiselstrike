---
# Settings the slug to / defines the home
sidebar_position: 1
---
# Getting Started

This is a basic ChiselStrike tutorial.  It describes what ChiselStrike
is, what it can do for you, and how to make it do various useful
things.  To achieve this, the tutorial shows small working examples
that illustrate important bits of functionality.

ChiselStrike is a backend generator.  It provides instant backend
functionality when you need it.  The main use-case it addresses is when
you need a backend endpoint for your dynamic site, but you don't want
to bother implementing an entire backend server.  For example, let's
say you have a site page that shows all comments written by site
users.  Because comments are dynamically generated on the site itself
(not from your CMS), you need a dynamic backend service (and
_endpoint_, in our lingo) to call.

If you had such an endpoint available, what would it look like?  Let's
examine that first, even before we have the endpoint.  We can say, for
instance, that it's an http URL to which you can send a GET operation, and
it will respond with a JSON array of all comments.

[//]: # (TODO: We should paginate by default.)

If we had such and endpoint right now, we could interact with it via
`curl`, like this:

```bash
curl localhost:8080/dev/comments
```

You are expected to see:

```output
curl: (7) Failed to connect to localhost port 8080: Connection refused
```

Obviously, we get "Connection refused", since ChiselStrike isn't
active yet.  Let's change that: in another window, type this:

```bash
npx create-chiselstrike-app my-backend
```

and you will see a new ChiselStrike project being generated:

```console
Creating a new ChiselStrike project in my-backend ...
Installing packages. This might take a couple of minutes.

added 25 packages, and audited 26 packages in 8s

3 packages are looking for funding
  run `npm fund` for details

found 0 vulnerabilities
```

You can then start ChiselStrike in local development mode with:

```bash
cd my-backend
npm run dev
```

and see the ChiselStrike start up:

```
> hello@1.0.0 dev
> chisel dev

🚀 Thank you for your interest in the ChiselStrike beta! 🚀

⚠️  This software is for evaluation purposes only. Do not use it in production. ⚠️

📚 Docs:    https://docs.chiselstrike.com
💬 Discord: https://discord.gg/4B5D7hYwub
📧 Email:   beta@chiselstrike.com

For any question, concerns, or early feedback, please contact us via email or Discord!

INFO - ChiselStrike is ready 🚀 - URL: http://localhost:8080
End point defined: /dev/hello
```

This starts ChiselStrike on your localhost.  It will continue running
and dynamically loading files in the `my-backend` directory when they
change.  To stop it, run `pkill chisel` in a terminal.  For full
reference of `chisel` command usage, please see [this
page](Reference/chisel-cli) or run `chisel --help`.

## Generating Endpoints

Now that ChiselStrike is running, we can attempt to access our
endpoint again:

```bash
curl -f localhost:8080/dev/comments
```

and see `curl` output the following:

```console
curl: (22) The requested URL returned error: 404
```

Hey, this is progress -- at least the connection is accepted now! :)
But the ChiselStrike backend responds with 404, since our endpoint
hasn't been defined yet.  That's OK, though: defining an endpoint is
easy.  We do it by adding a TypeScript file under the
`my-backend/endpoints` directory.  Here is one:

```typescript title="my-backend/endpoints/comments.ts"
import { responseFromJson } from "@chiselstrike/api"

export default function chisel(_req) {
    return responseFromJson("Temporarily empty");
}
```

When you save this file, you'll see this line in the `chisel dev`
output:

```
End point defined: /dev/comments
```

That's all it takes to define an endpoint!  It is now ready for use,
which you can again verify with `curl`:

```bash
curl localhost:8080/dev/comments
```

to see `curl` output the following:

```
"Temporarily empty"
```

As you can see, ChiselStrike reads your TypeScript and turns it into
backend functionality that is available immediately.

:::note
You may think that ChiselStrike executes your TypeScript verbatim, but
that is not necessarily what happens.  ChiselStrike has a builtin
compiler that lets it parse and transform your endpoint definition
into any form that is equivalent.  You focus on describing the
endpoint functionality in whatever way is most convenient for you;
ChiselStrike will make sure it runs well.
:::

Let's say a few words about the code in `comments.ts`.  The first
thing you'll notice is that it exports a function named `chisel` with
a single parameter.  This function defines the endpoint.  It takes a
[Request](https://developer.mozilla.org/en-US/docs/Web/API/Request)
and returns the corresponding
[Response](https://developer.mozilla.org/en-US/docs/Web/API/Response).
In this case, it simply returns a string wrapped as a JSON value.  It
uses a helper function `responseFromJson`. There's much more
to `@chiselstrike/api`, as we'll see shortly.  For full
reference, please see [this page](data-access).

## Adding Data

So how can we make the endpoint dynamic?  How do we leverage the
ChiselStrike backend to store our comments and serve them to us when
necessary?  This is where backend models come in -- you can describe to
ChiselStrike the data you want it to store for you by defining some
TypeScript classes.  Put a file in `my-backend/models/models.ts` like this:

```typescript title="my-backend/models/models.ts"
import { ChiselEntity } from "@chiselstrike/api"

export class BlogComment extends ChiselEntity {
    content: string = "";
    by: string = "";
}
```

:::tip
You are able to specify default values in your type properties, like you would for a normal typescript
class. Properties can be added or removed as you go if they have default values, so it is always recommended
you add them.
:::

When you save this file, you should see this line in the `chisel dev`
output:

```
Model defined: BlogComment
```

What this does is define an entity named `BlogComment` with one string
field named `content`.  ChiselStrike will process this and begin
storing `BlogComment` objects in its database.

:::tip
By default, ChiselStrike doesn't check your types (we assume your IDE did that for you!), which results
in faster loading of your endpoints. Our cli can bundle type checking by calling `tsc` directly, which can
be achieved by passing the `--type-check` option to `npm run dev`, or to the apply command `npx chisel apply`
:::

Now we need to expose those entities through an API. The simplest possible API is simply a [RESTful API](https://en.wikipedia.org/wiki/Representational_state_transfer), a standard that describes how an endpoint can handle various HTTP verbs
to provide basic operations on a collection of entities: create, read,
update, and delete ([CRUD](https://en.wikipedia.org/wiki/Create,_read,_update_and_delete)).

ChiselStrike makes that as easy as it gets. To generate our RESTful API, including a `POST` method
so we can add data to the database, add the following file:

```typescript title="my-backend/endpoints/comments.ts"
import { BlogComment } from "../models/models";
export default BlogComment.crud();
```

Upon saving this file, there will be an endpoint in ChiselStrike
for us to call:

```bash
curl -X POST -d '{"content": "First comment", "by": "Jill"}' localhost:8080/dev/comments
curl -X POST -d '{"content": "Second comment", "by": "Jack"}' localhost:8080/dev/comments
curl -X POST -d '{"content": "Third comment", "by": "Jim"}' localhost:8080/dev/comments
curl -X POST -d '{"content": "Fourth comment", "by": "Jack"}' localhost:8080/dev/comments
curl -X POST -d '{"content": "Wrong comment", "by": "Author"}' localhost:8080/dev/comments
```

Each of them will return an output, for example:

```json
{"id":"a4ca3ab3-2e26-4da6-a5de-418c1e6b9b83","content":"First comment","by":"Jill"}
```
:::tip
Note how you do not need to specify an `id` for the `BlogComment` entity. An `id` property is automatically generated for you.
:::

Now we just have to read them. Because our `crud` function also registers a `GET` handler,
we can just issue:

```bash
curl localhost:8080/dev/comments | python -m json.tool
```

to get all the Comments:

```json
[
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
  },
  {
    "id": "d419e629-4304-44d5-b534-9ce446f25e9d",
    "content": "Wrong comment",
    "by": "Author"
  }
]
```

or specify an id:

```bash
curl localhost:8080/dev/comments/a4ca3ab3-2e26-4da6-a5de-418c1e6b9b83 | python -m json.tool
```

```json
{
  "id": "a4ca3ab3-2e26-4da6-a5de-418c1e6b9b83",
  "content": "First comment",
  "by": "Jill"
}
```

The API also allows you to filter by specific properties, by specifying a search parameter with a partial URL-encoded JSON object:

```bash
curl -g localhost:8080/dev/comments?f={%22by%22:%22Jack%22} | python -m json.tool
```

```json
[
  {
    "id": "fed312d7-b36b-4f34-bb04-fba327a3f440",
    "content": "Second comment",
    "by": "Jack"
  },
  {
    "id": "5bfef47e-371b-44e8-a2dd-88260b5c3f2c",
    "content": "Fourth comment",
    "by": "Jack"
  }
]
```

We can also amend an object with `PUT`:

```
curl -X PUT -d '{"content": "Right Comment", "by": "Right Author"}' localhost:8080/dev/comments/d419e629-4304-44d5-b534-9ce446f25e9d
```

and ultimately `DELETE` it:

```
curl -X DELETE localhost:8080/dev/comments/d419e629-4304-44d5-b534-9ce446f25e9d
```

CRUD generation is infinitely customizable; please see its JSDoc for
an extensive description.  Here is an example that forbids DELETE,
POST, and PUT while extending the GET result with either `{"data":
VALUE}` or `{"error": "message"}`:

```typescript title="my-backend/endpoints/comments-readonly.ts"
import { crud, standardCRUDMethods, responseFromJson } from "@chiselstrike/api";
import { BlogComment } from "../models/models";
export default crud(
    BlogComment,
    ":id", /* :id can be explicitly provided */
    {
        customMethods: {
            DELETE: standardCRUDMethods.methodNotAllowed,
            POST: standardCRUDMethods.methodNotAllowed,
            PUT: standardCRUDMethods.methodNotAllowed,
        },
        createResponses: {
            GET: (body: unknown, status: number) => {
                if (status < 400) {
                    return responseFromJson({ data: body }, status);
                }
                return responseFromJson({ error: body }, status);
            },
        }
    },
);
```

## Beyond CRUD

Being able to just get started very quickly and spawn a CRUD API is great, but as your
project evolves in complexity you may find yourself needing custom logic.

ChiselStrike allows each `endpoint` file to export a default method that takes a [Request](https://developer.mozilla.org/en-US/docs/Web/API/Request)
object as a parameter, and returns a [Response](https://developer.mozilla.org/en-US/docs/Web/API/Response). You can then use
whatever logic you want.

:::tip
Changes to the backend cannot happen during a `GET` request. Make sure that if you are making changes to the backend state,
they happen under `PUT`, `POST`, or `DELETE`!
:::

Now let's change our endpoint's code:

```typescript title="my-backend/endpoints/comments.ts"
import { responseFromJson } from "@chiselstrike/api"
import { BlogComment } from "../models/models"

export default async function chisel(req) {
    if (req.method == 'POST') {
        const payload = await req.json();
        const content = payload["content"] || "";
        const by = payload["by"] || "anonymous";
        const created = BlogComment.build({content, by});
        await created.save();
        return responseFromJson('inserted ' + created.id);
    } else if (req.method == 'GET') {
        const comments = await BlogComment.cursor().select("content", "by").toArray();
        return responseFromJson(comments);
    } else {
        return new Response("Wrong method", { status: 405 });
    }
}
```

:::tip
Remember how we didn't have to specify an `id` in the model? We can now access it
as `created.id` in the example above. If the object doesn't have an `id`, one is created for you upon
`save`. If it does, `save()` will override the corresponding object in the backend storage
:::

In this endpoint, we're now getting to know ChiselStrike's API and runtime better. Notice how
we were able to parse the request under `POST` with our own custom validation, and then use
the `build` API to construct an object that is then persisted with `save`.

During `GET`, we can acquire a `cursor()`, `select()` which properties we want, and then
transform it to a standard Javascript `Array`. If we didn't need to do this slicing, we could
have used the convenience function `findMany()`.

Lastly, notice how we can return a standard `Response`, but also invoke the convenience method
`responseFromJson` where we know the result is a JSON object.

Let's now invoke it with:

```bash
curl -X POST -d '{"content": "Fifth comment", "by": "Jill"}' localhost:8080/dev/comments
```

to see `curl` report:

```console
"inserted 78604b77-7ff1-4d13-a025-2b3aa9a4d2ef"
```

We can then run the following `curl` command:

```bash
curl -s localhost:8080/dev/comments | python -m json.tool
```

and see the following:

```json
[
    {
        "content": "First comment",
        "by": "Jill"
    },
    {
        "content": "Second comment",
        "by": "Jack"
    },
    {
        "content": "Third comment",
        "by": "Jim"
    },
    {
        "content": "Fourth comment",
        "by": "Jack"
    },
    {
        "content": "Fifth comment",
        "by": "Jill"
    }
]
```

🎉 Ta-da! You're a pro now! From generating a simple CRUD RESTful API, to a custom endpoint that behaves
exactly how you need it to, you've done it all!

## File-based routing

Like [Gatsby](https://www.gatsbyjs.com/docs/reference/routing/creating-routes/#define-routes-in-srcpages) and
[NextJS](https://nextjs.org/docs/routing/introduction#nested-routes), ChiselStrike routes incoming requests by
matching the URL path against the endpoint-code path.  When you create a file `endpoints/posts.ts`, the URL
`/dev/posts` invokes it.  When you create a file `endpoints/new/york/city.ts`, the URL `/dev/new/york/city`
invokes it.  But what happens when there is no exact match?  In that case, ChiselStrike uses the longest
prefix of the URL path that matches an existing endpoint definition.  In the previous example, the URL
`/dev/new/york/city/manhattan/downtown` will also be handled by `endpoints/new/york/city.ts` (assuming no
other endpoints).

This routing procedure helps the RESTful API work correctly.  For example, the above endpoint
`my-backend/endpoints/comments.ts` will be invoked when you access a specific comment, eg, at
`/dev/comments/1234-abcd-5678-efgh`.  The `BlogComment.crud()` will parse the URL and understand that a single
collection element is being accessed.
