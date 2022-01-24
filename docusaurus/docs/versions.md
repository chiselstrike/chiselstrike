---
sidebar_position: 3
---
# API Versioning

Now that we have defined our endpoints, models, and policies, it is time to spice things up.
You may have noticed that all endpoints created by ChiselStrike have `/dev/` as part of the route.
That's because ChiselStrike makes API versioning a first-class citizen: everything you deploy to
ChiselStrike is deployed as part of a _version_.

## Creating a new version

Versions are an optional parameter to the `chisel apply` command.

:::tip
Versioning is a production-oriented workflow.

When starting to play with versions, it is advisable that you turn off `chisel dev`, as it will
keep trying to push changes to the `dev` version. Start your server with `chisel start` instead,
and push your changes manually with `chisel apply`.
:::

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
$ curl localhost:8080/experimental/comments
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

:::info Feedback Requested! We could use your help!
The next version of our beta will allow you to populate your new version, as well as linking different versions together
and propagating changes.

* How important for you is to populate from an existing version?
  * Do you want to populate only a portion of your production data?
* How important for you is to populate from fake data generators?
  * Do you have integrations you would like to see supported?

* How would you like to specify links between versions to aid your production experience? Examples include
  * In a yaml file in your git repository?
  * With a Typescript file (like calling a function) in your git repository?
  * In the command-line?
  * Through a JSON endpoint?

* If two versions contain incompatible models, we will allow you to specify a function with a transformation.
  * I would prefer to write this file in Typescript.
  * I would prefer to write this file in yaml, and only support simple property mappings.
  * I would prefer to write this file in yaml covering the simple cases, but embed a Typescript expression for the complex cases.
  * 🤢 Please anything but yaml.
:::

### Use-cases for API versioning

API versioning is useful for:
* Development/preview branches,
* Supporting older and newer clients, once version linking in supported.

