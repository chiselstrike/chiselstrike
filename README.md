![banner](imgs/logo.jpg)

---

[![Build Status](https://github.com/chiselstrike/chiselstrike/workflows/Rust/badge.svg?event=push&branch=main)](https://github.com/chiselstrike/chiselstrike/actions)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue)](https://github.com/chiselstrike/chiselstrike/blob/master/LICENSE)
[![Discord](https://img.shields.io/discord/933071162680958986?color=5865F2&label=discord&logo=discord&logoColor=8a9095)](https://discord.gg/GHNN9CNAZe)
[![Twitter](https://img.shields.io/twitter/follow/chiselstrike?style=plastic)](https://twitter.com/chiselstrike)

## Quick start

To get a CRUD API working in 30 seconds or less, first create a new project:

```console
npx -y create-chiselstrike-app my-app
cd my-app
```

Add a model, by writing the following TypeScript code to the `models/BlogComment.ts`:

```typescript
cat << EOF >models/BlogComment.ts
import { ChiselEntity } from "@chiselstrike/api"

export class BlogComment extends ChiselEntity {
    content: string = "";
    by: string = "";
}
EOF
```

Add an endpoint, by writing the following TypeScript code to the `endpoints/comments.ts`:

```typescript
cat << EOF >endpoints/comments.ts
import { BlogComment } from "../models/BlogComment";
export default BlogComment.crud();
EOF
```

Start the development server with:

```console
npm run dev
```

And there you go, you can now do:
```console
curl -X POST -d '{"content": "First comment", "by": "Jill"}' localhost:8080/dev/comments
```

and you just inserted your first blog post!


## What is ChiselStrike?

ChiselStrike is a modern backend development framework for JavaScript/TypeScript applications. Quite a mouthful, eh?
Let's see what that means!

### Is ChiselStrike a database?

No! The [founding team at ChiselStrike](https://www.chiselstrike.com/about) have written databases from scratch before, and
we believe there are better things to do in life, like pretty much everything else. ChiselStrike wraps around
existing databases, providing developers with a *relational-like abstraction* that allows one to think of backends
from the business needs down, instead of from the database up.

Instead, you can think of ChiselStrike as a big pool of global shared memory.
The data access API is an integral part of ChiselStrike and offers developers a way to just code, without
worrying about the underlying database - anymore than you worry about what happens in each level of the memory hierarchy.
(meaning some people do, but most people don't have to!)

### Is ChiselStrike an ORM?

Maybe? ChiselStrike has some aspects that overlap with traditional ORMs: it allows you to access database abstractions
in your programming language. However, in traditional ORMs you start from the database, and export it up. Changes
are done to the database schema, which is then bubbled up through *migrations*, and elements of the database invariably leak
to the API.

ChiselStrike, on the other hand, starts from your code and automates the decisions needed to implement that into the database.

Let's look at [ChiselStrike's documentation](https://docs.chiselstrike.com/Intro/first) for an example of what's needed to create a comment on a blog post:

```typescript
import { ChiselEntity } from "@chiselstrike/api"

export class BlogComment extends ChiselEntity {
    content: string = "";
    by: string = "";
}
```

The first thing you will notice is that there is no need to specify how those things map to the underlying database. No tracking
of primary keys, column types, etc.

Now imagine you need to start tracking whether this was created by a human or a bot. You can change your model
to say:

```typescript
import { ChiselEntity } from "@chiselstrike/api"

export class BlogComment extends ChiselEntity {
    content: string = "";
    by: string = "";
    isHuman: boolean = false;
}
```

and that's it! there are no migrations, no need to alter table, or anything like that.

Furthermore, if you need to find all blog posts written by humans (soon to return very few entries), you
can just write a lambda, instead of trying to craft a database query in TypeScript:

```typescript
const all = await BlogComment.findMany(p => p.isHuman);
```

### Is ChiselStrike a TypeScript runtime?

ChiselStrike includes a TypeScript runtime - the fantastic and beloved [Deno](https://github.com/denoland/deno). That's the last piece of the puzzle
with the data API and the database bundles. That allows you to develop everything locally from your laptop and integrate
with your favorite frontend framework. Be it Next.js, Gatsby, Remix, we're cool with any of them!

![](imgs/diagram.png)

### That's all fine and all, but I need more than that!

I heard you: no modern application is complete without authentication and security. ChiselStrike integrates with [next-auth](https://next-auth.js.org/)
and allows you to specify authentication entities directly from your TypeScript models!

You can then add a policy file that details which fields can be accessed, and which endpoints are available.

For example, you can store the blog authors as part of the models,

```typescript
import { ChiselEntity, AuthUser } from "@chiselstrike/api"

export class BlogComment extends ChiselEntity {
    content: string = "";
    @labels("protect") author: AuthUser;
}
```

and then write a policy saying that the users should only be able to see the posts that they themselves
originated:

```yaml
labels:
  - name: protect
    transform: match_login
```

Now your security policies are declaratively applied separately from the code, and you can easily grasp what's
going on.

### So in summary?

ChiselStrike provides everything you need to handle your backend, from the data layer to the business logic, wrapped
in powerful abstractions that let you just code and not worry about handling databases schemas, migrations, and operations
again.

It allows you to declaratively specify compliance policies around who can access the data and under which circumstances.

Your ChiselStrike files can go into their own repo, or even better, into a subdirectory of your existing frontend repo. You can
code your presentation and data layer together, and turn any frontend framework into a full-stack (including the database layer!)
framework in no time.


## Contributing

To build and develop from source:

```console
git submodule update --init --recursive
cargo build
```

That will build the `chiseld` server and `chisel` utility.

You can now use `npx` to install a local version of the API:
```console
npx ./packages/create-chiselstrike-app --chisel-version="file:../packages/chiselstrike-api" my-backend
```

And then replace instances of `npm run` with direct calls to the new binaries. For example, instead of
`npm run dev`, run

```console
cd my-backend
../target/debug/chisel dev
```


Also, consider:

[Open (or fix!) an issue](https://github.com/chiselstrike/chiselstrike/issues) 🙇‍♂️

[Join our discord community](https://discord.gg/GHNN9CNAZe) 🤩

[Start a discussion](https://github.com/chiselstrike/chiselstrike/discussions/) 🙋‍♀️


## Next steps?

Our documentation, including a quick tutorial, is available [here](https://docs.chiselstrike.com)

Our hosted version is available [here](https://www.chiselstrike.com/login) and is now in beta. You can deploy your project
straight from your git repository and see your backend materializing in front of your eyes.
