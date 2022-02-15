---
sidebar_position: 7
---

# Advanced data modeling

We have so far seen how to write simple models in ChiselStrike. Our goal is to make the code
feel as native as possible for TypeScript users, and we aim to derive as much as possible of
the backend from your TypeScript definitions.

However, there are some things that have semantic meaning that is cannot be easily derived
from code. For those, we rely on annotations to allow you to tell us how to behave.

We have already seen one example: The `labels` decorator is used to tell ChiselStrike about the
semantic meaning of your properties so we can, for example, anonymize them or automatically filter.

There is, at the moment, an extra semantic decorator that you can use, but more are planned in the future.

## Uniqueness

By using the `@unique` decorator, you can let ChiselStrike know that a certain property is
unique. For type evolution, the `@unique` decorator can be removed from a field, but it cannot
be added.

Here is one example of it in practice:

```typescript title="models/post.ts"
import { ChiselEntity, unique } from "@chiselstrike/api"

export class BlogPost extends ChiselEntity {
    @unique relUrl: string;
    content: string;
}
```

We can now code an endpoint that will store a post, with a given
relative URL:

```typescript title="endpoints/post.ts"
import { BlogPost } from "../models/post";
import { responseFromJson } from "@chiselstrike/api";

export default async function chisel(req) {
    if (req.method == 'POST') {
        const payload = await req.json();
        const content = payload["content"] ?? "";
        // mandatory!
        const relUrl = payload["relUrl"];
        if (relUrl === undefined) {
            return new Response("Missing relUrl", { status: 400 });
        }
        const created = BlogPost.build({content, relUrl});
        await created.save();
        return responseFromJson('inserted ' + created.id);
    } else if (req.method == 'GET') {
        const comments = await BlogPost.findMany({});
        return responseFromJson(comments);
    } else {
        return new Response("Wrong method", { status: 405 });
    }
}
```

And as you can see, we can add a post:
```
curl -d '{ "relUrl": "post.html", "content": "We at ChiselStrike are so happy to have you with us!" }' -X POST http://localhost:8080/dev/post
```

```
"inserted 9cb079dd-abfe-488c-9def-c9439c2d80f4"
```

Read the list of posts:
```
curl http://localhost:8080/dev/post
```

```json
[
  {
    "id": "9cb079dd-abfe-488c-9def-c9439c2d80f4",
    "relUrl": "post.html",
    "content": "We at ChiselStrike are so happy to have you with us!"
  }
]
```

But upon trying to execute the same command as before with the same relative URL, we get a status code `500`
