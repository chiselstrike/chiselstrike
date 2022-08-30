# Data Access

We've already previewed working with data in [Getting Started](first). Let's explain the data system a bit more.

## Defining Models

Models represent the domain objects of your application.

For example, in a blogging platform, you will have entities such as `BlogPost`, `BlogComment`, `Author`, and so on.

To define a `User`, you can add the following TypeScript class to a file in the `models/` directory:

```typescript  title="my-backend/models/User.ts"
import { ChiselEntity } from "@chiselstrike/api"

export class User extends ChiselEntity {
    username: string;
    email: string;
    city: string;
}
```

and another example:

```typescript title="my-backend/models/models.ts"
import { ChiselEntity, labels } from "@chiselstrike/api"
import { User } from "../models/User"

export class BlogComment extends ChiselEntity {
    content: string = "";
    author: User = new User();
}
```

The ChiselStrike runtime will detect changes in the `models/` directory and make any neccessary adjustments to the underlying backing datastore.

## Saving Objects

The `ChiselEntity` base class that our `User` entity extends provides a `save()` method that will save an object to the datastore.

Here is an example endpoint demo:

<!-- FIXME : update the example below to return JSON -->
```typescript title="my-backend/routes/create.ts"
import { responseFromJson } from "@chiselstrike/api"
import { User } from "../models/User"

export default async function (req) {
  const payload = await req.json();
  const username = payload["username"] || "";
  const email = payload["email"] || "";
  const city = payload["city"] || "";
  const user = User.build({ username, email, city });
  await user.save();
  return user;
}
```

We can now create a user using a POST request:

```bash
curl -d '{"username": "alice", "email": "alice@example.com", "city": "Cambridge" }' localhost:8080/dev/create
```

and we'll get the following response:

<!-- FIXME : JSON -->

```console
{"username": "alice", "email": "alice@example.com", "city": "Cambridge", "id": "72325865-1887-4604-a127-025919ca281c" }
```

As discussed in the [Getting Started](Intro/first.md) section, the ChiselStrike runtime assigns an `id` to your entity automatically upon `save()`. If you want to _update_ your entity, you need know its `id`.  The ID can be returned when you create the object, or you can query for it.

<!-- FIXME: need a Section "Updating Objects" -->
<!-- FIXME: need a Section "Deleting Objects" -->

Still, you are not technically limited to making every HTTP route follow REST principles by using ids. For example, you could write the following 'update' route that recieves the same JSON, but finds the `User` entity based on the provided `username`:

```typescript title="my-backend/routes/update.ts"
import { responseFromJson } from "@chiselstrike/api";
import { User } from "../models/User";

export default async function (req) {
  const payload = await req.json();
  const username = payload["username"] || "";
  const email = payload["email"] || "";
  const city = payload["city"] || "";
  const id = payload["id"];
  let user = id
    ? User.build({ id, username, email, city })
    : await User.findOne({ username });

  if (!user) {
    return new Response("id not provided and user " + username + " not found");
  } else {
    user.email = email;
    user.city = city;
    await user.save();
    return responseFromJson("Updated " + user.username + " id " + user.id);
  }
}
```

## Querying Single Objects

In some of the above examples, we've previewed how to query objects using the `User.findOne()` method call.

There are two search methods `findOne()` and `findMany()` for querying.

For example, to query one entity with a given `username`, we could use the following code:

```typescript title="my-backend/routes/find-one.ts"
import { responseFromJson } from "@chiselstrike/api"
import { User } from "../models/User"

export default async function (req) {
  const payload = await req.json();
  const user = await User.findOne(payload) ?? "Not found";
  return responseFromJson(user);
}
```

and query it with `POST /dev/find-one`:

```bash
curl -d '{ "email": "alice@mit.edu" }' localhost:8080/dev/find-one
```

and see `curl` report:

```console
"Found alice"
```

## Querying Multiple Objects

To find multiple entities, use the `findMany()` method:

```typescript title="my-backend/routes/find-many.ts"
import { responseFromJson } from "@chiselstrike/api"
import { User } from "../models/User"

export default async function (req) {
  const payload = await req.json();
  const user = await User.findMany(payload);
  return responseFromJson('Found ' + user.map(user => user.username));
}
```

and query it with `POST /dev/find-many`:

```bash
curl -d '{ "city": "Cambridge" }' localhost:8080/dev/find-many
```

which returns:

```console
"Found alice"
```

We can create more entities with:

```bash
curl -d '{"username": "bob", "email": "bob@example.com", "city": "Cambridge" }' localhost:8080/dev/create
```

We can then invoke the `POST /dev/find-many` route again:

```bash
curl -d '{ "city": "Cambridge" }' localhost:8080/dev/find-many
```

which returns additional results:

```console
"Found alice,bob"
```

:::note
`findMany` can be called with a predicate lambda as well:

```typescript title="my-backend/routes/find-many.ts"
import { responseFromJson } from "@chiselstrike/api"
import { User } from "../models/User"

export default async function (req) {
  const user = await User.findMany(user => user.city == "Cambridge");
  return responseFromJson('Found ' + user.map(user => user.username));
}
```
:::

You can also pass an empty restrictions object to `findMany()` and you will get all the entities of that type.

To do that, invoke the `POST /dev/find-many` route with an empty JSON document:

```bash
curl -d '{}' localhost:8080/dev/find-many
```

and see `curl` report:

<!-- FIXME : make these all JSON -->

```
"Found alice,bob"
```

:::note
The `findMany()` method is convenient, but if there are too many results, this can consume a lot of time and memory.
In future releases of ChiselStrike, the runtime will enforce a maximum number of entities from `findMany()` at API level
and pagination in result sets will be available for REST-API consumers.
:::

<!-- FIXME: expand explanation here, possibly a different page even -->

## Updating Objects

The documentation robots are at work. Examples coming soon!

## Deleting Objects

Entities are deleted using the `ChiselEntity.delete(restriction)` method. For
example, with the `User` entity defined earlier, you delete an entity as follows:

```typescript title="my-backend/routes/delete.ts"
import { User } from "../models/User.ts"

export default async function (req: Request) {
    const payload = await req.json()
    const email = payload.email;
    await User.delete({ email });
    return new Response("Deleted " + email);
}
```

In this example, we delete an user by their email address.

## Relationships

Let's look closer at the BlogComment example we gave earlier:

```typescript title="my-backend/models/models.ts"
import { ChiselEntity, labels } from "@chiselstrike/api"
import { User } from "../models/User"

export class BlogComment extends ChiselEntity {
    content: string = "";
    author: User = new User();
}
```

You can observe that the `author` field is of type `User`, which is another entity. This is a way we can define a relationship between entities.

Entity fields are eagerly loaded. This means that when you load a `BlogComment` instance, the author field entity is loaded with it. Same goes for saving -- when you save a `BlogComment` with an author, a `User` instance will be upserted in the database and reference to it will be associated with the stored `BlogComment`. (So if this `User` already existed in the database, it will be updated with the value provided.)


## See Also: Cursors

Now you've seen all the basics about data-access and hope you are enjoying not having to write any SQL or deal with migrations or anything like that!
We have some additional options available. When you feel like exploring, read [Cursors](InDepth/cursors.md) for how to build queries in very powerful composable ways.


