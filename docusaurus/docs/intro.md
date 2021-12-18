---
# Settings the slug to / defines the home
slug: /
sidebar_position: 1
---
# Introduction to ChiselStrike

This is a basic ChiselStrike tutorial.  It describes what ChiselStrike
is, what it can do for you, and how to make it do various useful
things.  To achieve this, the tutorial shows small working examples
that illustrate important bits of functionality.

:::note
We assume here that you are self-hosting ChiselStrike.  You should
have received from us a package with executable programs `chisel` and
`chiseld`.  Keep both in the same directory, and ensure this directory
is in your PATH.
:::

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
$ curl localhost:8080/dev/comments
curl: (7) Failed to connect to localhost port 8080: Connection refused
```

Obviously, we get "Connection refused", since ChiselStrike isn't
active yet.  Let's change that: in another window, type this:

```
$ chisel new my-backend
Initialized ChiselStrike project in my-backend
$ cd my-backend
$ chisel dev
🙇‍♂️ Thank you for your interest in the ChiselStrike private beta! (Beta-Jan22.1)
⚠️  This is provided to you for evaluation purposes and should not be used to host production at this time
Docs with a description of expected functionality and command references at https://docs.chiselstrike.com
For any question, concerns, or early feedback, contact us at beta@chiselstrike.com

🎅 Happy Holidays! 🎄

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
$ curl -f localhost:8080/dev/comments
curl: (22) The requested URL returned error: 404
```

Hey, this is progress -- at least the connection is accepted now! :)
But the ChiselStrike backend responds with 404, since our endpoint
hasn't been defined yet.  That's OK, though: defining an endpoint is
easy.  We do it by adding a TypeScript file under the
`my-backend/endpoints` directory.  Here is one:

```typescript title="my-backend/endpoints/comments.ts"
export default function chisel(_req) {
    return Chisel.json("Temporarily empty");
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
$ curl localhost:8080/dev/comments
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
uses a helper function `Chisel.json` that comes from an object named
`Chisel` that's available to each endpoint's code.  There's much more
to `Chisel` than `Chisel.json`, as we'll see shortly.  For full
reference, please see [this page](chisel-backend).

## Adding Data

So how can we make the endpoint dynamic?  How do we leverage the
ChiselStrike backend to store our comments and serve them to us when
necessary?  This is where backend types come in -- you can describe to
ChiselStrike the data you want it to store for you by defining some
TypeScript types.  Put a file in `my-backend/types/t.ts` like this:

```typescript title="my-backend/types/t.ts"
class Comment {
    content: string;
    by: string;
}
```

When you save this file, you should see this line in the `chisel dev`
output:

```
Type defined: Comment
```

:::info Feedback Requested! We could use your help!
Currently, we don't support relations (types as part of other types). We expect
to add support for that in the next version of the beta.

* Would you prefer to just add a property that references another type, or provide type decorators to guide the process?
* What kind of complicated relationships do you want to handle, and what is challenging about them in your current solutions?
:::

:::tip
You are able to specify default values in your type properties, like you would for a normal typescript
class. Types can be added or removed as you go if they have default values, so it is always recommended
you add them.
:::

What this does is define an entity named `Comment` with one string
field named `content`.  ChiselStrike will process this and begin
storing `Comment` objects in its database.  To populate it, add the
following file:

```typescript title="my-backend/endpoints/populate-comments.ts"
export default async function chisel(_req) {
    for (const c of [
        {content: 'First comment', by: 'Jill'},
        {content: 'Second comment', by: 'Jack'},
        {content: 'Third comment', by: 'Jim'},
        {content: 'Fourth comment', by: 'Jack'}
    ]) {
        await Chisel.store('Comment', c);
    }
    return new Response('success\n');
}
```

Upon saving this file, there will be another endpoint in ChiselStrike
for us to call:

```bash
$ curl localhost:8080/dev/populate-comments
success
```

Note how we can store a comment in the database by simply invoking
`Chisel.store` with `'Comment'` as the first argument and the object
representing the comment as the second.  Every time we do that, a new
row is added.

The effect of this endpoint is that the database is filled with four
comments.  Now we just have to read them.  Let's edit the
`my-backend/endpoints/comments.ts` file as follows:

```typescript title="my-backend/endpoints/comments.ts"
export default async function chisel(_req) {
    let comments = [];
    for await (let c of Comment) {
        comments.push(c);
    }
    return Chisel.json(comments);
}
```

:::tip
You do not need to specify an id for `Comment`. An `id` property is automatically generated for you, and
you can access it as `c.id` in the examples above. Calling `Chisel.store()` passing an object that has an
`id` will update the field with corresponding object.
:::

Note that we changed `chisel` to an async function.  This is because
it uses the `for await` construct to go over all the stored comments.
What makes it easy is that ChiselStrike defines the variable `Comment`
(corresponding to the type `Comment` from t.ts), which is a collection
of all the instances of this type that ChiselStrike has in data
storage.  Now we can call this endpoint to see the comments we stored:

```bash
$ curl -s localhost:8080/dev/comments | python -m json.tool
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
