# Cursors

As shown in [Data Access](Intro/data-access.md), the `findOne` and `findMany()` methods are convenient, but for more advanced use, ChiselStrike
provides a cursor API for building queries.

This composable system also means that you can even write functions that build up queries programmatically
and pass them around as arguments.

The `ChiselEntity` base class provides a `cursor()` method to obtain a `ChiselCursor`.  The `ChiselCursor` class provides a variety of composable operations, such as `filter()`, `take()`, `select()`, 
for building queries.

For example, the `findOne()` example could be written using the cursor-based API as:

```typescript title="my-backend/endpoints/find-one-cursor.ts"
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
| `filter(restrictions)`| Restrict this cursor to contain only entities matching the given `restrictions`. |
| `forEach(function)`   | Execute `function` for every entity in this cursor. |
| `select(...fields)`   | Return another cursor with a projection of each entity by `fields`.      |
| `take(count)`         | Take `count` entities from this cursor. |
| `toArray()`           | Convert this cursor to an array.  |

<!-- FIXME : without examples it's unclear what a restrictions object or a function predicate is, this needs a simpler explanation with examples. -->

:::note
The `ChiselCursor` interface is still evolving. For example, methods such as `skip()`,  `map()`, and `reduce()` are planned for future releases.
:::

### `filter`

ChiselCursor supports two versions of the `filter` method. The first accepts a predicate identifying elements to be kept or ignored. As an example, let's find all Gmail users:

```typescript
  const gmailUsers = await User.cursor()
      .filter((user: User) => user.email.endsWith("@gmail.com"));
```

The second overload takes a restrictions-object parameter. It allows you to filter by *equality* based on an object whose keys correspond to attributes of an Entity matching on respective values. For example, let's find Alice by email:

```typescript
  const users = await User.cursor().filter({"email": "alice@mit.edu"});
```

## Notes On Transactions

ChiselStrke currently implements implicit transactional evaluation. A transaction is created before ChiselStrike
starts evaluating your endpoint and is automatically committed after your endpoint ends and we generate
the HTTP response. In case your endpoint returns a stream, any database-related operation done within
stream-generation code will happen outside of the transaction and can result in a crash.

If your code crashes or explicitly throws an exception that is not caught, ChiselStrike rolls back the
transaction automatically.

Explicit user-controlled transactions are coming soon!
