---
# Settings the slug to / defines the home
sidebar_position: 1
---
# Getting Started

This is a basic ChiselStrike tutorial, where we'll show how to build some simple
endpoints for your data-driven web applications. 

ChiselStrike comes in two parts - the backend generator (available in npm) and
our service platform for hosting ChiselStrike data apps in production.

This tutorial will show you how to use the generator, which allows for easy local
development and testing.

Say you're just starting out building a new application for a dynamic site, but you don't want
to bother implementing an entire backend server, configuring a SQL database, and all
of that.

A good example might be building a blog that allows comments. Because comments are dynamically generated on the site itself
(not from your CMS), you need a dynamic backend service with a REST API endpoint to call.

If we had such and endpoint right now, we could interact with it via
`curl`, like this:

```bash
curl localhost:8080/dev/comments
```

Of course, we haven't done anything yet. Let's get started!

```bash
npx create-chiselstrike-app my-backend
```

This will install all dependencies and you will see a new ChiselStrike project being generated:

```console
Creating a new ChiselStrike project in my-backend ...
Installing packages. This might take a couple of minutes.

added 25 packages, and audited 26 packages in 8s

3 packages are looking for funding
  run `npm fund` for details

found 0 vulnerabilities
```

You can then start ChiselStrike in local development mode by running ChiselStrike in a new
terminal tab:

```bash
cd my-backend
npm run dev
```

You will see output as the ChiselStrike server starts up on localhost:

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
change.  

For more about `chisel` command usage, please see [this
page](Reference/chisel-cli) or run `chisel --help`.

## Generating Endpoints

To make our endpoint for "/dev/commands", we add a TypeScript file
in the `my-backend/endpoints` directory.  Here is one:

```typescript title="my-backend/endpoints/comments.ts"
import { responseFromJson } from "@chiselstrike/api"

export default function chisel(_req) {
    return responseFromJson("Comments go here!");
}
```

Once you save this file, you'll see output in the `chisel dev`
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
"Comments go here!"
```

As you can see, ChiselStrike reads your TypeScript and turns it into
backend functionality that is available immediately!

In the next step, we'll make our endpoint connect to some data.

Let's say a few words about the code we wrote above for `comments.ts`.  The first
thing you'll notice is that it exports a function named `chisel` with
a single parameter.  This function defines the endpoint.  It takes a
[Request](https://developer.mozilla.org/en-US/docs/Web/API/Request)
and returns the corresponding
[Response](https://developer.mozilla.org/en-US/docs/Web/API/Response).
In this above example, it simply returns a string wrapped as a JSON value.  It
uses a helper function `responseFromJson`. 

There's much more to `@chiselstrike/api`, as we'll see shortly.  For full
reference, please see [this page](data-access).

## Adding Data

It's time to be able to save and load comments.

This is where backend models come in -- models use typescript to describe
the data you want to store.  

Create a file in `my-backend/models/models.ts` like this:

```typescript title="my-backend/models/models.ts"
import { ChiselEntity } from "@chiselstrike/api"

export class BlogComment extends ChiselEntity {
    content: string = "";
    by: string = "";
}
```

:::tip
By default, `chisel` doesn't check your TypeScript types (we assume your IDE did that for you!), which results
in faster production code. If you want type checking, you can enable it by calling `tsc` directly, which can
be achieved by passing the `--type-check` option to `npm run dev`, or to the apply command `npx chisel apply`
:::

Here we have defined a comment which has a string and an author name.

:::tip
You are able to specify default values in your type properties, like you would for a normal typescript
class. Properties can be added or removed as you go if they have default values, so it is always recommended
you add them.
:::

When you save this file, you should see new output from the `chisel dev` command that remains running to
compile your work.

```
Model defined: BlogComment
```

Now you are able to store `BlogComment` objects.

Now we need to surface those entities in our API.  We're big fans of [REST](https://en.wikipedia.org/wiki/Representational_state_transfer), but don't strictly require it. 
If you're not familiar, REST is a set of practices that describes how an endpoint can handle various HTTP verbs
to provide ways to manipulate a collection of entities: create, read,
update, and delete ([CRUD](https://en.wikipedia.org/wiki/Create,_read,_update_and_delete)).

ChiselStrike makes REST as easy as it gets. To generate a REST collection for BlogComment, including a `POST` method
so we can add comments to the database, we can create the following endpoints file:

```typescript title="my-backend/endpoints/comments.ts"
import { BlogComment } from "../models/models";
export default BlogComment.crud();
```

Wow that was short! After saving this file, there will be an endpoint in ChiselStrike
for us to try out!

```bash
curl -X POST -d '{"content": "First comment", "by": "Jill"}' localhost:8080/dev/comments
curl -X POST -d '{"content": "Second comment", "by": "Jack"}' localhost:8080/dev/comments
curl -X POST -d '{"content": "Third comment", "by": "Jim"}' localhost:8080/dev/comments
curl -X POST -d '{"content": "Fourth comment", "by": "Jack"}' localhost:8080/dev/comments
curl -X POST -d '{"content": "Wrong comment", "by": "Author"}' localhost:8080/dev/comments
```

Each POST will return a response to the caller with the object ID, for example:

```json
{"id":"a4ca3ab3-2e26-4da6-a5de-418c1e6b9b83","content":"First comment","by":"Jill"}
```
:::tip
Note how you do not need to specify an `id` for the `BlogComment` entity. An `id` property is automatically generated for you on all objects.
We always use UUIDs rather than integers.
:::

Now that we've inserted some objects, lets read them back! Our `crud` function also registers a `GET` handler, so this is also already available!

...note:
Pagination support for collections of large objects will be coming very soon!
...

```bash
curl localhost:8080/dev/comments | python -m json.tool
```

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

To get a specific comment, we can just specify an id:

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

CRUD generation is customizable; more detail around security policy is coming soon but 
here is a lower-level example that forbids DELETE, POST, and PUT while wrapping the GET result 
with either `{"data": VALUE}` or `{"error": "message"}` depending on the result.

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
project evolves in complexity you may find yourself needing custom business logic and endpoints
that don't fit neatly into REST workflows.

ChiselStrike allows each `endpoint` file to export a default method that takes a [Request](https://developer.mozilla.org/en-US/docs/Web/API/Request)
object as a parameter, and returns a [Response](https://developer.mozilla.org/en-US/docs/Web/API/Response). You can then add whatever logic you want.

This is a lower level mechanism and is pretty raw, we are working on syntax features that will make this much more powerful.

:::tip
You can't change data during a `GET` request. Make sure that if you are making changes to the backend state,
they happen under `PUT`, `POST`, or `DELETE`!
:::

Now let's edit our endpoint's code to show off a "full customization" example.

```typescript title="my-backend/endpoints/comments.ts"
import { responseFromJson } from "@chiselstrike/api"
import { BlogComment } from "../models/models"

export default async function chisel(req) {

    if (req.method == 'POST') {
        const payload = await req.json();
        const by = payload["by"] || "anonymous";
        const created = BlogComment.build({'content': payload['content'], 'by': by });
        await created.save();
        return responseFromJson(created);
    }

    else if (req.method == 'GET') {
        const tokens = req.url.split('/')
        // better syntax around this coming soon!
        if (tokens.length != 6) {
           const comments = await BlogComment.cursor().toArray();
           return responseFromJson(comments);
        } else {
           const id = tokens.reverse()[0]
           const comment = await BlogComment.findOne({id})
           if (comment) {
              return responseFromJson(comment)
           }
           return new Response("Not found", { status: 404 })
        }
    }

    else {
        return new Response("Wrong method", { status: 405 });
    }
}

```

:::tip
Remember how we didn't have to specify an `id` in the model? We can now access it
as `created.id` in the example above. If the object doesn't have an `id`, one is created for you upon
`save`. If it does, `save()` will override the corresponding object in the backend storage
:::

:::tip
Notice that right now using the API to access objects that do not exist returns null values, rather
than raising exceptions. This may change in the near future, though right now we do our own explicit
error checking
:::

With this endpoint example, we're now getting to know ChiselStrike's API and runtime better. Notice how
we were able to parse the request under `POST` with our own custom validation, and then use
the `build` API to construct an object that is then persisted with `save`.

During `GET`, we can acquire a `cursor()`, `select()` which properties we want, and then
transform it to a standard Javascript `Array`. If we didn't need to do this slicing, we could
have used the convenience function `findMany()`.

Finally, notice how we can return a standard `Response` in some cases, but also can also invoke the convenience method
`responseFromJson` where we know the result is a JSON object.

Let's now test our endpoint with a POST:

```bash
curl -X POST -d '{"content": "Fifth comment", "by": "Jill"}' localhost:8080/dev/comments
```

and `curl` should return something like:

```console
{"id":"7190f1c5-7b81-4180-9db5-2d9c6ce17d6d","content":"Fifth comment","by":"Jill"}
```

Now lets fetch the entire list of comments:


```bash
curl -s localhost:8080/dev/comments | python -m json.tool
```

and we should see something like the following:

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

(We've left off the UUIDs above)


🎉 Ta-da! You're a pro now! From generating a simple CRUD RESTful API, to a custom endpoint that behaves
exactly how you need it to, you've now learned a large chunk of the ChiselStrike platform!

## Additional Notes on File-based routing

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


