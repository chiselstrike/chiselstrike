# Routing

Like [Gatsby](https://www.gatsbyjs.com/docs/reference/routing/creating-routes/#define-routes-in-srcpages) and
[NextJS](https://nextjs.org/docs/routing/introduction#nested-routes), ChiselStrike routes incoming requests by
matching the URL path against the files in the `routes/` directory. However, like
[Express.js](https://expressjs.com/en/guide/routing.html), ChiselStrike also supports programmatic definition
of routes.

For example, suppose we have the following code in `routes/user.ts`:

```typescript title="my-backend/routes/user.ts"
import { RouteMap, ChiselRequest } from "@chiselstrike/api";

export default new RouteMap()
    .get("/:id", function (req: ChiselRequest) {
        return `Profile for user ${req.params.get("id")}`;
    })
    .get("/:id/comments", function (req: ChiselRequest) {
        return `Comments by user ${req.params.get("id")}`;
    });
```

When ChiselStrike receives an HTTP request like `GET /dev/user/123/comments`, it is processed as follows:

- The `/dev` part specifies that the request is handled by the `dev` version.
- The remainder of the path, `/user/123/comments` in our case, is then matched against the files in the
    `routes` directory. In this example, this means that we invoke the file `user.ts`.
- We then look at the `RouteMap` exported from `user.ts` and match the rest of the path, `/123/comments`, to
    the defined routes. The first matching route is the `.get("/:id/comments", ...)` call, so we invoke the
    request handler and obtain the response: `Comments by user 123`.

## File-based routing

We map the files in the `routes` directory to path patterns as follows:

- We only consider files that end with `.ts` (we report an error if we find a `.js` file).
- Names of directories and files are treated as literal matches.
- A file named `index.ts` defines a route for the directory that it is placed in.
- Dynamic path segments (directories or files) are defined using brackets `[` and `]`.

Here are some examples of file names and the corresponding path patterns:

| File | Path pattern |
| ---- | ------------ |
| `user.ts` | `/user` |
| `user/comments.ts` | `/user/comments` |
| `user/index.ts` | `/user` |
| `index.ts` | `/` |
| `user/[id].ts` | `/user/:id` |
| `user/[id]/comments.ts` | `/user/:id/comments` |

## `RouteMap`

Every file in the `routes` directory should export a `RouteMap` using `default export`. The `RouteMap` defines
routes relative to the file. In the example above, the file `routes/user.ts` exports a `RouteMap` with two
routes, `GET /:id` and `GET /:id/comments`, so the full routes (including the path to the file) will be `GET
/user/:id` and `GET /user/:id/comments`.

:::note
For backward compatiblity, the files in the `routes` directory can also export an async function that will
directly handle all requests. This functionality is deprecated and will eventually be removed, new code should
always export a `RouteMap`.
:::

The methods `RouteMap.get(path, handler)` and `RouteMap.post(path, handler)`, which we have used in the
examples so far, are just shorthands for the more general method `RouteMap.route(method, path, handler)`:

```typescript
// the shorthand method `get()`...
.get("/user/:id", handler)

// ...is equivalent to this call of `route()`:
.route("GET", "/user/:id", handler)
```

You can use the `route()` method to register a handler that is invoked for multiple methods:

```typescript
// register the same handler for PUT and POST:
.route(["PUT", "POST"], "/user/:id", handler)

// register the handler for all HTTP methods:
.route("*", "/user/:id", handler)
```

## `RouteMap` composition

As an advanced feature, you may even embed a `RouteMap` inside other `RouteMap`, using the method
`RouteMap.prefix(path, routeMap)`:

```typescript
// routes that deal with users
const userMap = new RouteMap()
    .get("/:id", getUser)
    .get("/:id/avatar", getUserAvatar)
    .post("/:id", postUser);

// routes that deal with blogs
const blogMap = new RouteMap()
    .get("/:slug", getBlog)
    .get("/:slug/contents", getBlogContents)
    .patch("/:slug", patchBlog);

new RouteMap()
    // register all routes from `userMap` under prefix "/user"
    .prefix("/user", userMap)
    // same for `blogMap` with prefix "/blog"
    .prefix("/blog", blogMap)
```

:::tip Under the hood
In fact, this mechanism is also behind file-based routing. For example, when you create files `routes/user.ts`
and `routes/blog.ts`, we generate code like the following to build the "root" `RouteMap` that is used to
dispatch all requests in your backend:

```javascript
import routeUser from "./routes/user.ts";
import routeBlog from "./routes/blog.ts";

const routeMap = new RouteMap();
routeMap.prefix("/user", routeUser);
routeMap.prefix("/blog", routeBlog);
export default routeMap;
```
:::
