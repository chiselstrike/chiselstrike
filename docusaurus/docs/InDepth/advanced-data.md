# Advanced Data Modeling

So far, we have shown how to write simple models in ChiselStrike. Our goal is to make the code
feel as native as possible for TypeScript users, and we aim to build as much as possible of
the backend from just your TypeScript definitions.

Still, there are some things that have cannot be easily derived
from just the code. For those, we rely on annotations to allow you to tell our system how to behave.

We have already seen one example: The `labels` decorator is used to tell ChiselStrike about the
semantic meaning of your properties so we can, for example, anonymize them or automatically filter results.

There is, at the moment, one more semantic decorator `unique` that you can use, but more are planned in the future.

## Uniqueness

By using the `@unique` decorator, you can let ChiselStrike know that a certain property is
unique. For type evolution purposes, the `@unique` decorator can be removed from a field, but it cannot
be added later after a field is already defined.

Here is one example of it in practice:

```typescript title="my-backend/models/BlogPost.ts"
import { ChiselEntity, unique } from "@chiselstrike/api"

export class BlogPost extends ChiselEntity {
    @unique relUrl: string;
    content: string;
}
```

We can now code an endpoint that will store a post, but posts must
have unique URLs:

```typescript title="endpoints/post.ts"
import { BlogPost } from "../models/BlogPost";
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
        return created;
    } else if (req.method == 'GET') {
        const comments = await BlogPost.findMany({});
        return comments;
    } else {
        return new Response("Wrong method", { status: 405 });
    }
}
```

And as you can see, we can add a post:

```
curl -d '{ "relUrl": "post.html", "content": "We at ChiselStrike are so happy to have you with us!" }' -X POST http://localhost:8080/dev/post
```

And we can get a post...

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

<!-- possibly should be HTTP 409 which indicates a user fault -->

## Evolution

Sometimes, we get things wrong or add software features and would like our models to evolve. The aim of ChiselStrike is to allow for
model evolution without any database migration. While fully arbitrary evolution is not here yet, there are
many cases that we can already handle. You will see that they cover many scenarios, especially if you
prepare in advance. They are:

* Fields that have a default value can always be added or removed.
* Fields that are optional can always be added or removed.

Going back ot our `BlogPost` model, notice that if we try to add another field, we will see an error messsage:

```typescript title="my-backend/models/BlogPost.ts"
import { ChiselEntity, unique } from "@chiselstrike/api"

export class BlogPost extends ChiselEntity {
    @unique relUrl: string;
    content: string;
    newField: string;
}
```

Our logs will show:

```
unsafe to replace type: BlogPost. Reason: Trying to add a new non-optional field (newField) without a default value. Consider adding a default value or making it optional to make the types compatible
```

It is possible, however, to add:

```typescript title="my-backend/models/BlogPost.ts"
import { ChiselEntity, unique } from "@chiselstrike/api"

export class BlogPost extends ChiselEntity {
    @unique relUrl: string;
    content: string;
    newField?: string;
}
```

And after that:

```typescript title="my-backend/models/BlogPost.ts"
import { ChiselEntity, unique } from "@chiselstrike/api"

export class BlogPost extends ChiselEntity {
    @unique relUrl: string;
    content: string;
    newField?: string;
    newerField: boolean = false;
}
```
