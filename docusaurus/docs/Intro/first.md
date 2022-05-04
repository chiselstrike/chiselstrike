# Getting Started

In this introductory ChiselStrike tutorial, we'll show how to build some simple
endpoints for your data-driven web applications. Along the sidebar at your left,
you'll see more advanced topics  you can explore as you are interested in them.

ChiselStrike comes in two parts - the backend generator `chisel` (available in npm) and
our platform that hosts your ChiselStrike data apps in production.

This tutorial will show you how to use `chisel`, which allows for easy local
development and testing.

Imagine you're just starting out building a new application for a dynamic site, but you don't want
to bother implementing an entire backend server, configuring a SQL database, and managing the deployment
for it.

One of the simplest examples might involve building a blog that allows readers to make comments.
Even if a blog articles were statically rendered, the comment section would need some kind of
dynamic endpoint to make it work.

# Setup

Let's get started by using ChiselStrike to create a skeleton of a backend project. This
step will also install all our dependencies.

```bash
npx create-chiselstrike-app my-backend
```

:::tip Node Version?
You need Node 14.18.0 or later installed to successfully run the command.
:::

:::info Are you on Windows??
At the moment, ChiselStrike is supported on Windows through WSL.
Aside from that, on WSL2 you should create your project in an ext4 filesystem (like the `$HOME` folder) to support hot reloading
of endpoints. See details [here](https://stackoverflow.com/a/70275534)
:::

Output will look something like this:

```console
Creating a new ChiselStrike project in my-backend ...
Installing packages. This might take a couple of minutes.

added 25 packages, and audited 26 packages in 8s

3 packages are looking for funding
  run `npm fund` for details

found 0 vulnerabilities
```

You can then start ChiselStrike in local development mode by running ChiselStrike in a new
terminal tab. As ChiselStrike runs, it will compile your work automatically as you
make changes. It is also hosting your endpoints with a local development server at the same time.

```bash
cd my-backend
npm run dev
```

You will see output like this:

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

...tip:
For more about `chisel` command usage, please see [the CLI reference](InDepth/chisel-cli.md) or run `chisel --help`.
...

## Our First Endpoint

To make our endpoint for "/dev/comments", we create a TypeScript file
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
which you can check with `curl`:

```bash
curl localhost:8080/dev/comments
```

and should see the following output:

```
"Comments go here!"
```

In the next step, we'll make our endpoint connect to some data.

What happened in the above example? The first
thing you'll notice is that the endpoint file `comments.ts` exports a function named `chisel` with
a single parameter.  This function defines the logic for the endpoint.  It takes a
[Request](https://developer.mozilla.org/en-US/docs/Web/API/Request)
and returns the corresponding
[Response](https://developer.mozilla.org/en-US/docs/Web/API/Response).
In this above example, we simply returns a string wrapped as a JSON value. Where it is obvious
that an object is being returned (this will be explained soon), explicit calls to `responseFromJson`
are not needed.

## Our First Model

Next, let's add the ability to save and load comments.

First, we need to define what types of data we are going to save and load.
This is where backend models come in -- models use Typescript to describe
what kind of data you want to store.

Create a file in `my-backend/models/BlogComment.ts`:

```typescript title="my-backend/models/BlogComment.ts"
import { ChiselEntity } from "@chiselstrike/api"

export class BlogComment extends ChiselEntity {
    content: string = "";
    by: string = "";
}
```

Here we have defined a `BlogComment` which has a string `content` and an author name `by`.

<!-- FIXME: Move this to some advanced tips and tricks section, probably, to not distract from the tutorial? -->

:::tip
You are able to specify default values for fields, like you would for a normal typescript
class. Properties can be added or removed over time if they have default values, so it is always recommended
you add them.
:::

:::tip
Since we're using TypeScript, you may have some questions about type checking. By default, `chisel` doesn't check
your TypeScript types (we assume your IDE did that for you!), which results
in faster production code. If you want type checking, you can enable it by calling `tsc` directly, which can
be achieved by passing the `--type-check` option to `npm run dev`, or to the apply command `npx chisel apply`
:::

Once you save this file, you should see new output from the `chisel dev` command that remains running to
compile your work and serve up your endpoints:

```
Model defined: BlogComment
```

Now you are able to store `BlogComment` objects!  However, we still need to surface those entities through a web-services API endpoint.
That comes next!

## Combining Endpoints And Models

We're big fans of [REST](https://en.wikipedia.org/wiki/Representational_state_transfer), but don't strictly require it in ChiselStrike.

If you're not familiar, REST is a set of practices that describes how a URL endpoint can handle various HTTP verbs
to provide ways to manipulate a collection of entities: create, read,
update, and delete ([CRUD](https://en.wikipedia.org/wiki/Create,_read,_update_and_delete)).

ChiselStrike makes REST as easy as it gets. To generate a REST collection for BlogComment, including a `POST` method
so we can add comments to the database, we can create the following endpoints file:

```typescript title="my-backend/endpoints/comments.ts"
import { BlogComment } from "../models/BlogComment";
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
Note how you do not need to specify an `id` for the `BlogComment` entity in the POST. An `id` property is automatically generated for you on all objects.
We always use UUIDs rather than integers.
:::

:::tip
Right now you are testing only locally, but you'll want to think about restricting access to some endpoints in production.
We'll talk about security more in the [Policy](InDepth/pol.md) section.
:::

Now that we've inserted some objects, lets read them back! Our `crud` function also registers a `GET` handler, which is already available!

```bash
curl localhost:8080/dev/comments
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

...note:
Obviously, If we had 10,000 blog responses we wouldn't want to return them all at once.
Pagination support for collections of large objects will be coming very soon!
...

To get a specific comment, we can specify an id in the URL:

```bash
curl localhost:8080/dev/comments/a4ca3ab3-2e26-4da6-a5de-418c1e6b9b83
```

```json
{
  "id": "a4ca3ab3-2e26-4da6-a5de-418c1e6b9b83",
  "content": "First comment",
  "by": "Jill"
}
```

# Built-In Search

The API allows you to filter by object properties. For example:

```bash
curl -g localhost:8080/dev/comments?.by=Jack
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

will return all comments where field `by` is equal to `Jack`. Our api supports other comparison operators as well. For example
`curl -g localhost:8080/dev/comments?.by~like=Ji%25` will in our example return all comments by Jim and Jill (`%25` is encoded wildcard `%`). We support the following comparators:

| symbol      | Description |
| ----------- | ----------- |
|             | If no comparator is specified, the filter will check for equality |
| ~ne         | Not equal |
| ~lt         | Lower than |
| ~lte        | Lower than or equal |
| ~gt         | Greater than |
| ~gte        | Greater than or equal |
| ~like       | Like operator - supports the same syntax as SQL Like operator |
| ~unlike    | Equivalent to SQL's NOT LIKE |

Relationships are supported as well. Imagine that Comments's field `by` would be of type `Person` which would have a field `age`. In such a scenario, to get all comments that were written byt authors under 40 and are named John, we would do:

```bash
curl -g localhost:8080/dev/comments?.by.age~lt=40&.by.name=John
```

Similarly, you can order the results by specifying the `sort` parameter:
```bash
curl -g localhost:8080/dev/comments?sort=-by
```

```json
[
  {
    "id": "adc89862-dfaa-43ab-a639-477111afc55e",
    "content": "Third comment",
    "by": "Jim"
  },
  {
    "id": "a4ca3ab3-2e26-4da6-a5de-418c1e6b9b83",
    "content": "First comment",
    "by": "Jill"
  },
  {
    "id": "5bfef47e-371b-44e8-a2dd-88260b5c3f2c",
    "content": "Fourth comment",
    "by": "Jack"
  },
  {
    "id": "fed312d7-b36b-4f34-bb04-fba327a3f440",
    "content": "Second comment",
    "by": "Jack"
  },
  {
    "id": "d419e629-4304-44d5-b534-9ce446f25e9d",
    "content": "Wrong comment",
    "by": "Author"
  }
]
```

Note the minus `-` sign in front of the field name `by`. It signifies a descending sort ordering.
For ascending order, you use a `+` prefix or omit it completely which will default to ascending.

...tip:
When using the ascending ordering with prefix `+`, your HTTP library may do URL encoding automatically, but if it doesn't, `+` needs to be encoded as `%2B`.
...

To limit the result set to only the first `n` elements, you can use the the `limit` parameter:
```bash
curl -g localhost:8080/dev/comments?sort=by&limit=3
```

```json
[
    {
    "id": "d419e629-4304-44d5-b534-9ce446f25e9d",
    "content": "Wrong comment",
    "by": "Author"
  },
  {
    "id": "fed312d7-b36b-4f34-bb04-fba327a3f440",
    "content": "Second comment",
    "by": "Jack"
  },
    {
    "id": "5bfef47e-371b-44e8-a2dd-88260b5c3f2c",
    "content": "Fourth comment",
    "by": "Jack"
  },
]
```

To skip the first `n` elements, you can use the `offset` parameter:
```bash
curl -g localhost:8080/dev/comments?sort=by&offset=4
```

```json
[
  {
    "id": "adc89862-dfaa-43ab-a639-477111afc55e",
    "content": "Third comment",
    "by": "Jim"
  },
]
```

...note:
If both `limit` and `offset` are used, they are applied in traditional order - we first skip all elements up to the `offset` and then we return `limit` number of remaining elements.
...

...note:
The order in which you specify CRUD parameters *does not* matter. For example `?sort=by&limit=2&sort=content` will yield the same results as `?sort=content&limit=2`.
...

## PUT and DELETE

We can also amend an object with `PUT`:

```
curl -X PUT -d '{"content": "Right Comment", "by": "Right Author"}' localhost:8080/dev/comments/d419e629-4304-44d5-b534-9ce446f25e9d
```

<!-- FIXME: add an example about PATCH? -->

and ultimately `DELETE` it:

```
curl -X DELETE localhost:8080/dev/comments/d419e629-4304-44d5-b534-9ce446f25e9d
```

Alternatively, you can delete by specifying the same filters as for GET method. So for example, to delete all Comments written by Jack, we can write:

```
curl -X DELETE localhost:8080/dev/comments/?.by=Jack
```

🎉 Ta-da! You're a pro now!  There's a basic simple CRUD RESTful API, right out of the box.
In the next sections we'll show how to customize this endpoint further.

