---
sidebar_position: 2
---
# Data Access

We're already previewed working with data in [Getting Started](getting started). Let's explain the data system a bit more.

## Defining Models

Models represent the domain objects of your application.

For example, in a blogging platform, you will have entities such as `BlogPost`, `BlogComment`, `Author`, and so on.

To define a `User`, you can add the following TypeScript class to a file in the `models/` directory:

```typescript title="models/models.ts"
import { ChiselEntity, labels } from "@chiselstrike/api"

export class BlogComment extends ChiselEntity {
    content: string = "";
    @labels("pii") by: string = "";
}

export class User extends ChiselEntity {
    username: string;
    email: string;
    city: string;
}
```

The ChiselStrike runtime will detect the change in the `models/` directory and makes any neccessary adjustments to the underlying backing datastore.

## Saving Objects

The `ChiselEntity` base class that our `User` entity extends provides a `save()` method that will save an object to the datastore.
Here is an example endpoint demo:

<!-- FIXME : update the example below to return JSON -->
```typescript title="endpoints/create.ts"
import { responseFromJson } from "@chiselstrike/api"
import { User } from "../models/models"

export default async function (req) {
  const payload = await req.json();
  const username = payload["username"] || "";
  const email = payload["email"] || "";
  const city = payload["city"] || "";
  const user = User.build({ username, email, city });
  await user.save();
  return responseFromJson('Created user ' + user.username + ' with id ' + user.id);
}
```

We can now create a user through a REST post!:

```bash
curl -d '{"username": "alice", "email": "alice@example.com", "city": "Cambridge" }' localhost:8080/dev/create
```

and we'll get the following response:

<!-- FIXME : JSON -->

```console
"Created user alice with id 72325865-1887-4604-a127-025919ca281c"
```

As discussed in the [Getting Started](/first.md) section, the ChiselStrike runtime assigns an `id` to your entity automatically upon `save()`. If you want to _update_ your entity, you need know its `id`.  The ID can be returned when you create the object, or you can query for it.

<!-- FIXME: need a Section "Updating Objects" -->
<!-- FIXME: need a Section "Deleting Objects" -->

Still, you are not technically limited to making every endpoint speak follow REST principles by using ids. For example, you could write the following 'update' endpoint that recieves the same JSON, but finds the `User` entity based on the provided `username`:

```typescript title="endpoints/update.ts"
import { responseFromJson } from "@chiselstrike/api";
import { User } from "../models/models";

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

For example, to query one entity with a given `username`, we could use the following example code in an endpoint:

```typescript title="endpoints/find-one.ts"
import { responseFromJson } from "@chiselstrike/api"
import { User } from "../models/models"

export default async function (req) {
  const payload = await req.json();
  const user = await User.findOne(payload) ?? "Not found";
  return responseFromJson(user);
}
```

and query it with `/dev/find-one`:

```bash
curl -d '{ "email": "alice@mit.edu" }' localhost:8080/dev/find-one
```

and see `curl` report:

```console
"Found alice"
```

## Querying Multiple Objects

To find multiple entities, use the `findMany()` method:

```typescript title="endpoints/find-many.ts"
import { responseFromJson } from "@chiselstrike/api"
import { User } from "../models/models"

export default async function (req) {
  const payload = await req.json();
  const user = await User.findMany(payload);
  return responseFromJson('Found ' + user.map(user => user.username));
}
```

and query it with `/dev/find-many`:

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

We can then invoke the `/dev/find-many` endpoint again:

```bash
curl -d '{ "city": "Cambridge" }' localhost:8080/dev/find-many
```

which returns additional results:

```console
"Found alice,bob"
```

You can also pass an empty restrictions object to `findMany()` and you will get all the entities of that type.

To do that, invoke the `/dev/find-many` test endpoint with an empty JSON document:

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

```typescript title="endpoints/find-one.ts"
object.delete()
```
Examples coming soon!

## Advanced Querying: Cursors

As shown above, the `findOne` and `findMany()` methods are convenient, but for more advanced use, ChiselStrike
provides a cursor API for building queries.

This composable system also means that you can even write functions that build up queries programatically
and pass them around as arguments.

The `ChiselEntity` base class provides a `cursor()` method to obtain a `ChiselCursor`.  The `ChiselCursor` class provides variety of composable operations, such as `filter()`, `take()`, `select()`, 
for building queries.

For example, the `findOne()` example could be written using the cursor-based API as:

```typescript title="endpoints/find-one-cursor.ts"
import { responseFromJson } from "@chiselstrike/api"
import { User } from "../models/models"

export default async function (req) {
  const payload = await req.json();
  const users = await User.cursor().filter(payload).take(1).toArray();
  return responseFromJson('Found ' + users.map(user => user.username));
}
```

You can invoke the `/dev/find-one-cursor` endpoint with:

```bash
curl -d '{ "email": "alice@mit.edu" }' localhost:8080/dev/find-one-cursor
```

and see `curl` report:

```console
"Found alice"
```

The methods provided by `ChiselCursor` are:

| Method                | Description |
| --------------------- | ----------- |
| `filter(predicate)`   | Restrict this cursor to contain only entities matching the given function `predicate`. |
| `filter(restriction)` | Restrict this cursor to contain only entities matching the given `restrictions`. |
| `forEach(function)`   | Execute `function` for every entity in this cursor. |
| `select(...fields)`   | Return another cursor with a projection of each entity by `fields`.      |
| `take(count)`         | Take `count` entities from this cursor. |
| `toArray()`           | Convert this cursor to an array.  |

<!-- FIXME : without examples it's unclear what a restriction object or a function predicate is, this needs a simpler explanation with examples. -->

:::note
The `ChiselCursor` interface is still evolving. For example, methods such as `skip()`,  `map()`, and `reduce()` are planned for future releases.
:::

### `filter`

ChiselCursor supports two versions of the `filter` method. The first accepts a predicate identifying elements to be kept or ignored. As an example, let's find all Gmail users:

```typescript
  const gmailUsers = await User.cursor()
      .filter((user: User) => user.email.endsWith("@gmail.com"));
```

The second overload takes a restriction-object parameter. It allows you to filter by *equality* based on an object whose keys correspond to attributes of an Entity matching on respective values. For example, let's find Alice by email:

```typescript
  const users = await User.cursor().filter({"email": "alice@mit.edu"});
```

## Transactions

ChiselStrke currently implements implicit transactional evaluation. A transaction is created before ChiselStrike
starts evaluating your endpoint and is automatically committed after your endpoint ends and we generate
the HTTP response. In case your endpoint returns a stream, any database-related operation done within
stream generation code will happen outside of the transaction and can result in a crash.

If your code crashes or explicitly throws exception that is not caught, ChiselStrike rollbacks the
transaction automatically.

Explicit user-controlled transactions are coming soon!
