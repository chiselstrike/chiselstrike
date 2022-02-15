---
sidebar_position: 8
---
# Known Issues

We put a lot of effort into getting this beta out the door soon, so we could
hear from you (yes, you!) about what you like and dislike in the product and
provide you with a great experience moving forward.

However, there are some issues that we plan to address soon but just didn't
make the cut. If you encounter any of them, just let us know and we'll do our
best to expedite it, but do know that they are planned functionality going forward.

* **Module imports:** ChiselStrike is built using Deno, which uses native browser-style URL
imports. However, it is consumable as a Node project, and you're more likely using VSCode.
So ChiselStrike won't accept an import like "@node-fetch" (with the exception of the `@chiselstrike`
family of imports, which are builtin), and VSCode will scream at an http-style browser imports.
There are also other potential issues with Node imports that are [well-known](https://deno.land/manual@v1.16.3/npm_nodejs/compatibility_mode).
While we do plan to provide you with a better experience in the future, for now if you do
want to use external modules, browser-style should work.

* **Joins:** We currently don't support explicit Joins. Implicitly the joins are partially supported
by nested Types (`class Y {z: int}; class X {y: Y}`). Support for explicit joins is coming soon.

* **Multi-file models:** All models have to go in the same file for now. We envision models
being put in different files (like `Person.ts`, `User.ts`, etc). But because those files are
essentially modules (and see the first bullet), this will only work at the moment if they are
all in the same file.

* **Changing types:** It is possible to evolve a model by adding and removing fields that have
default values without writing any migration file (how awesome is that???), but you can't
change types of existing fields. We intend to allow some of them to happen automatically, like
a number to a string, and some with a migration file (Don't worry! Pure TypeScript, no database knowledge needed!)
but that's not done yet.

* **Performance:** 🐌 Last, but not least, the deployment you are receiving is single-threaded, and we didn't
focus much on performance aside from designing a good architecture. So for now, don't expect any stellar
benchmark results, but we promise you, we'll get there! Hey, premature optimization is the root of all evil, isn't it??
Fun fact: this bullet point was supposed to come first, but due to its poor performance it ended up arriving
last.
