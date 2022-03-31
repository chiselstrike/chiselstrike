# More About Endpoints

In this section we'll show how to move beyond simple CRUD requests, as shown in [Getting Started](Intro/first.md).

# Customizing CRUD Further

<!-- FIXME: move into extra chapter? -->

CRUD generation is customizable; more detail and syntax around this and also security policy is coming soon but 
here is a lower-level example that forbids DELETE, POST, and PUT while wrapping the GET result 
with either `{"data": VALUE}` or `{"error": "message"}` depending on the result.

<!-- FIXME: replace with class based alternates once available -->

```typescript title="my-backend/endpoints/comments-readonly.ts"
import { crud, standardCRUDMethods, responseFromJson } from "@chiselstrike/api";
import { BlogComment } from "../models/BlogComment.ts";
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

You may also be interested in the [Authentication](InDepth/login.md) chapter.

## Full Custom Endpoints

Being able to just get started very quickly and spawn a CRUD API is great, but as your
project evolves in complexity you may find yourself needing custom business logic and endpoints
that don't fit neatly into REST workflows.

ChiselStrike allows each `endpoint` file to export a default method that takes a [Request](https://developer.mozilla.org/en-US/docs/Web/API/Request)
object as a parameter, and returns a [Response](https://developer.mozilla.org/en-US/docs/Web/API/Response). You can then add whatever logic you want.

This is a lower level mechanism and is pretty raw -- we are working on syntax features that will make this much more powerful.

:::tip
You can't change data during a `GET` request. Make sure that if you are making changes to the backend state,
they happen under `PUT`, `POST`, or `DELETE`!
:::

Now let's edit our endpoint's code to show off a "full customization" example.

```typescript title="my-backend/endpoints/comments.ts"
import { responseFromJson } from "@chiselstrike/api"
import { BlogComment } from "../models/BlogComment.ts"

export default async function chisel(req) {

    if (req.method == 'POST') {
        const payload = await req.json();
        const by = payload["by"] || "anonymous";
        const created = BlogComment.build({'content': payload['content'], by });
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
as `created.id` in the example above. If the object doesn't have an `id`, one is created for you after
`save`. 
:::

:::tip
Notice that right now using `findOne` to access an object that does not exist returns a null value, rather
than raising an error. This may change in the near future. We do our own explicit
error checking in this example.
:::

With this endpoint example, we're now getting to know ChiselStrike's API and runtime better. Notice how
we were able to parse the request under `POST` with our own custom validation, and then use
the `build` API to construct an object that is then persisted with `save`.  We'll explain the use of the 
data model more in [Data Access](Intro/data-access).

Finally, notice how we can return a standard `Response` in some cases, but also can also use the convenience method
`responseFromJson` where we know the result is a JSON object.

Let's now test our endpoint with a POST, and see it works similarly to the automatic "CRUD" example above.

```bash
curl -X POST -d '{"content": "Fifth comment", "by": "Jill"}' localhost:8080/dev/comments
```

and `curl` should return something like:

```console
{"id":"7190f1c5-7b81-4180-9db5-2d9c6ce17d6d","content":"Fifth comment","by":"Jill"}
```

Now lets fetch the entire list of comments:


```bash
curl -s localhost:8080/dev/comments
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


🎉 Nice! You've gone from a simple REST API for learning how to write full custom endpoints using the full data model.
It's time to explore our API in greater depth, then you can set out and explore other documentation sections according
to your interests!

