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

🙇‍♂️ Thank you for your interest in the ChiselStrike private beta! (Beta-Jan22.2)
⚠️  This is provided to you for evaluation purposes and should not be used to host production at this time
Docs with a description of expected functionality and command references at https://docs.chiselstrike.com
For any question, concerns, or early feedback, contact us at beta@chiselstrike.com

 🍾 We hope you have a great 2022! 🥂

INFO - ChiselStrike is ready 🚀 - URL: http://127.0.0.1:8080
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
uses a helper function `responseFromJson` There's much more
to `responseFromJson` as we'll see shortly.  For full
reference, please see [this page](chisel-backend).

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

When you save this file, you should see this line in the `chisel dev`
output:

```
Model defined: BlogComment
```

:::info Feedback Requested! We could use your help!
Currently, we don't support relations (models as part of other models). We expect
to add support for that in the next version of the beta.

* Would you prefer to just add a property that references another type, or provide type decorators to guide the process?
* What kind of complicated relationships do you want to handle, and what is challenging about them in your current solutions?
:::

:::tip
You are able to specify default values in your type properties, like you would for a normal typescript
class. Properties can be added or removed as you go if they have default values, so it is always recommended
you add them.
:::

What this does is define an entity named `BlogComment` with one string
field named `content`.  ChiselStrike will process this and begin
storing `BlogComment` objects in its database.  To populate it, add the
following file:

```typescript title="my-backend/endpoints/populate-comments.ts"
import { BlogComment } from "../models/models";

export default async function chisel(_req) {
    let promises = [];
    promises.push(BlogComment.build({'content': "First comment", 'by': "Jill"}).save());
    promises.push(BlogComment.build({'content': "Second comment", 'by': "Jack"}).save());
    promises.push(BlogComment.build({'content': "Third comment", 'by': "Jim"}).save());
    promises.push(BlogComment.build({'content': "Fourth comment", 'by': "Jack"}).save());

    await Promise.all(promises);
    return new Response('success\n');
}
```

Upon saving this file, there will be another endpoint in ChiselStrike
for us to call:

```bash
curl -X POST localhost:8080/dev/populate-comments
```

and see `curl` output:

```
success
```

Note how we can store a comment in the database by simply invoking the `save`
method of `'BlogComment'`. Every time we do that for an object without an id, a
new row is added. Every time we invoke `save()` on an object that already has
an id, the corresponding row is updated.

Because this endpoint mutates the state of the backend, a consequence of
the call to `save()`, we use a POST request.

The effect of this endpoint is that the database is filled with four
comments.  Now we just have to read them.  Let's edit the
`my-backend/endpoints/comments.ts` file as follows:

```typescript title="my-backend/endpoints/comments.ts"
import { responseFromJson } from "@chiselstrike/api"
import { BlogComment } from "../models/models"

export default async function chisel(_req) {
    let comments = [];
    await BlogComment.cursor().forEach(c => {
        comments.push(c);
    });
    return responseFromJson(comments);
}
```

:::tip
You do not need to specify an id for `BlogComment`. An `id` property is automatically generated for you, and
you can access it as `c.id` in the examples above. Calling the `save()` on an object that already has an
`id` will update the field with corresponding object.
:::

Note that we changed `chisel` to an async function.  This is because
it uses the `forEach` method (which is an async method) to go over all the stored comments.
What makes it easy is that ChiselStrike defines the variable `BlogComment`
(corresponding to the type `BlogComment` from models.ts), which is a collection
of all the instances of this type that ChiselStrike has in data
storage.  Now we can call this endpoint to see the comments we stored:

```bash
curl -s localhost:8080/dev/comments | python -m json.tool
```

and see `curl` output:

```console
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
    }
]
```

Neat, they're all there! :)

## Accepting different HTTP methods in your endpoint


In ChiselStrike, changes to the backend cannot happen during a `GET` request.
We have so far seen an example of a POST endpoint, (populate) and an endpoint
that was effectively a GET through curl's default (comments), but didn't do
any checking to make sure it was the case. Furthermore, it is customary for
endpoints to accept both POST and GET requests on the same path and behave
accordingly, so we shouldn't need two different endpoints.

Now let's change that code to this:

```typescript title="my-backend/endpoints/comments.ts"
import { responseFromJson } from "@chiselstrike/api"
import { BlogComment } from "../models/models"

export default async function chisel(req) {
    if (req.method == 'POST') {
        const payload = await req.json();
        const content = payload["content"] || "";
        const by = payload["by"] || "anonymous";
        const created = BlogComment.build({'content': content, 'by': by});
        await created.save();
        return responseFromJson('inserted ' + created.id);
    } else if (req.method == 'GET') {
        let comments = [];
        await BlogComment.cursor().forEach(c => {
            comments.push(c);
        });
        return responseFromJson(comments);
    } else {
        return new Response("Wrong method", { status: 405 });
    }
}
```

And then invoke it with:

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

```console
[
    {
        "id": "5b415bff-2ea1-400f-89e2-f8c67494257e",
        "content": "First comment",
        "by": "Jill"
    },
    {
        "id": "8adb7fe6-9bdb-497a-bfe3-06180201e80f",
        "content": "Second comment",
        "by": "Jack"
    },
    {
        "id": "f52e5e98-626e-4534-9204-61a879c44c85",
        "content": "Third comment",
        "by": "Jim"
    },
    {
        "id": "4d8ab383-529c-4402-841e-06195915a285",
        "content": "Fourth comment",
        "by": "Jack"
    },
    {
        "id": "78604b77-7ff1-4d13-a025-2b3aa9a4d2ef",
        "content": "Fifth comment",
        "by": "Jill"
    }
]
```

🎉 Ta-da!  POST now inserts a comment, as you would expect.  The site can
now persist comments by sending POST requests to the `comments`
endpoint.
