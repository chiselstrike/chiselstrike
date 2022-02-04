---
sidebar_position: 2
---
# Entities and Queries

## Defining Entities

Entities represent the domain objects of your application.
For example, in a blogging platform, you will have entities such as `BlogPost`, `BlogComment`, `Author`, and so on.
The set of entities in your application represents the domain model, which is why in ChiselStrike, entities are defined in your project's `models` directory.

For example, to define an entity `User` that represents a user in your application, you can add the following TypeScript class to your existing models file:

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

:::note
All of your models have to be in the same file. We will lift this restriction
in the future, so each model can live in its own file.
:::

The ChiselStrike runtime picks up this entity definition in the `models` directory and automatically does the necessary adjustments to the underlying backing datastore so that the entity can be persisted.

## Persisting Entities

The `ChiselEntity` base class that our `User` entity extends provides a `save()` method, which you can use to persist your entity.

We can, for example, write the following endpoint that takes input as JSON, builds a `User` entity, and persists it with the `save()` method as follows:

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

You can now access the `/dev/create` endpoint:

```bash
curl -d '{"username": "alice", "email": "alice@example.com", "city": "Cambridge" }' localhost:8080/dev/create
```

to see `curl` report the following:

```console
"Created user alice with id 72325865-1887-4604-a127-025919ca281c"
```

Please note that, as discussed in the [Getting Started](intro.md) section, the ChiselStrike runtime assigns an `id` to your entity automatically upon `save()`. If you want to _update_ your entity, you need to either know its `id` from another object or external source or query it to obtain an entity with an `id`.

For example, you could write the following endpoint that takes the same JSON, but updates the `User` entity based on the provided `username`:

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

You can now update an entity using the `/dev/update` endpoint issuing a read-modify-write pattern:

```bash
curl -d '{"username": "alice", "email": "alice@mit.edu", "city": "Cambridge" }' localhost:8080/dev/update
```
or by explicitly mentioning the id:


```bash
curl -d '{"id": "72325865-1887-4604-a127-025919ca281c", "username": "alice", "email": "alice@mit.edu", "city": "Cambridge" }' localhost:8080/dev/update
```

which would both produce the following `curl` report:

```console
"Updated alice id 72325865-1887-4604-a127-025919ca281c"
```

## Querying Entities

We have now seen how to define entities and how to persist them, but also saw a glimpse of how to query them with the `User.findOne()` method call when we updated the entity.

The `ChiselEntity` base class provides two convenience methods, `findOne()` and `findMany()`, which you can use to query for entities of that type. Both of the method take an object as an argument, which represents the filtering restrictions.

For example, to query one entity with a given `username`, you could define the following endpoint:

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

To find multiple entities, you can use the `findMany()` method. For example, you can write the following endpoint:

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

and see `curl` report:

```console
"Found alice"
```

We can create more entities with:

```bash
curl -d '{"username": "bob", "email": "bob@example.com", "city": "Cambridge" }' localhost:8080/dev/create
```

and see `curl` report:

```console
"Created bob"
```

We can then invoke the `/dev/find-many` endpoint:

```bash
curl -d '{ "city": "Cambridge" }' localhost:8080/dev/find-many
```

To see that `findMany()` returns them if they match the restrictions:

```console
"Found alice,bob"
```

You can also pass an empty restrictions object to `findMany()` and you will get all the entities of that type.

To do that, invoke the `/dev/find-many` endpoint with an empty JSON document:

```bash
curl -d '{}' localhost:8080/dev/find-many
```

and see `curl` report:

```
"Found alice,bob"
```

:::note
The `findMany()` method is convenient, but also problematic if you have a lot of
entities stored because loading them can take a lot of time and memory. In future
releases of ChiselStrike, the runtime will enforce a maximum number of entities
`findMany()` can return and also enforce timeouts at the data store level. The
runtime will also provide optional pagination for the `findMany()` method.
:::

## Cursors

The `findOne` and `findMany()` methods are convenient, but the interface is not
composable, and can become hard to use for more complex queries. ChiselStrike
provides a cursor-based API for writing composable queries.

A cursor can be thought of as an index to an array of entities in the data store.
The `ChiselEntity` base class provides a `cursor()` method to obtain a `ChiselCursor`.
The `ChiselCursor` class provides variety of composable operations, such as `filter()`, `take()`, `select`, and so on for writing complex queries.
The actual query uses _deferred execution_, which allows the ChiselStrike runtime to optimize the query.

:::note
The ChiselStrike runtime does not perform query optimizations in the current release, but will do that in future releases.
:::

For example, the `findOne()` example could be written using the cursor-based API as follows:

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

The methods provided by `ChiselCursor` are outlined in the following table.

| Method                | Description |
| --------------------- | ----------- |
| `filter(restriction)` | Restrict this cursor to contain only entities matching the given `restrictions`. |
| `forEach(function)`   | Execute `function` for every entity in this cursor. |
| `join(right)`         | Join this cursor with the `right` cursor. |
| `select(...fields)`   | Return another cursor with a projection of each entity by `fields`.      |
| `take(count)`         | Take `count` entities from this cursor. |
| `toArray()`           | Convert this cursor to an array.  |

:::note
The `ChiselCursor` interface is still work-in-progress. For example, methods such as `skip()`,  `map()`, and `reduce()` are planned for future releases.
Also, the current implementation of `filter()` takes a _restriction object_, but future ChiselStrike runtimes will allow you to write filter functions using TypeScript, which are automatically converted to efficient database queries in many cases.
:::

## Transactions

We currently support implicit transactional evaluation. The transaction is created before ChiselStrike
starts evaluating your endpoint and is automatically committed after your endpoint ends and we generate
the HTTP response. In case your endpoint returns a stream, any database-related operation done within
stream generation code will happen outside of the transaction and can result in a crash.

If your code crashes or explicitly throws exception that is not caught, ChiselStrike rollbacks the
transaction automatically.

Explicit user-controlled transactions are coming soon.
