# API Versioning

Now that we have defined our endpoints, models, and policies, it is time to spice things up.
You may have noticed that all endpoints created by ChiselStrike have `/dev/` as part of the route.
That's because ChiselStrike makes API versioning a first-class citizen: everything you deploy to
ChiselStrike is deployed as part of a _version_.

## Creating a new version

:::tip
Versioning is a production-oriented workflow.

When starting to play with versions, it is advisable that you turn off `chisel dev`, as it will
keep trying to push changes to the `dev` version. Start your server with `chisel start` instead,
and push your changes manually with `chisel apply`.

If you installed througn `npm`, the `chisel` utility is available through `npx chisel`. Just
substitute `npx chisel` in the examples.
:::

Versions are an optional parameter to the `chisel apply` command.
Creating a version creates a fully independent branch of your backend.

Let's now create a new API version, called `experimental`. In the same directory,
type

```bash
chisel apply --version experimental
```

and `chisel apply` reports:

```console
Model defined: BlogComment
End point defined: /experimental/comments
```

Now let's try comparing the two endpoints.

If we invoke the new `experimental` API version:

```bash
curl localhost:8080/experimental/comments
```

The `curl` command reports no data:

```console
[]
```

However, we can still invoke the old `dev` version of the endpoint:

```bash
curl localhost:8080/dev/comments
```

and the `curl` command reports all the old data, untouched:

```console
[{"content":"First comment"},{"content":"Second comment"},{"content":"Third comment"},{"content":"Fourth comment"}]
```

The versions now can evolve independently.

## Populating from an existing version

Although you can create a new fully independent version and build it up by adding data
through your endpoints, it is sometimes useful to populate your new version from some
other existing version.

This can be done with `chisel populate`:

```bash
chisel populate --version experimental --from dev
```

Assuming `experimental` is empty before the population starts, you should see that the `experimental` version
now holding the same data as `dev`.

If you invoke the `/experimental/comments` endpoint:

```bash
curl localhost:8080/experimental/comments
```

The `curl` command reports:

```console
[{"content":"First comment"},{"content":"Second comment"},{"content":"Third comment"},{"content":"Fourth comment"}]
```

### Use-cases for API versioning

API versioning is useful for:
* Development/preview branches,
* Supporting older and newer clients, once version linking in supported.

