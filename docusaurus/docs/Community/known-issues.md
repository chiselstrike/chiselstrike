# Upcoming Items

We put a lot of effort into getting this beta out the door, so we could
hear from you (yes, you!) about what you like and dislike in the product and
provide you with a great experience moving forward.

Below, we wanted to share some known issues as well as upcoming features
and fixes not currently present in the beta.

<!-- FIXME: need to incorporate a feedback link into the docs -->

* **Module imports:** ChiselStrike is built using Deno, which uses native browser-style URL
imports. However, it is consumable as a Node project, and you're more likely using VSCode.
So ChiselStrike won't accept an import like "@node-fetch" (with the exception of the `@chiselstrike`
family of imports, which are builtin), and VSCode will scream at an http-style browser imports.
There are also other potential issues with Node imports that are [well-known](https://deno.land/manual@v1.16.3/npm_nodejs/compatibility_mode).
While we do plan to provide you with a better experience in the future, for now if you do
want to use external modules, browser-style should work.

* **Joins:** We currently don't support explicit Joins. Implicitly the joins are partially supported
by nested Types (`class Y {z: int}; class X {y: Y}`). Support for explicit joins is coming soon.

* **Syntactic Sugar:** We've shown some relatively low level examples with the Data API, and some very compelling class based
versions are in the works!

* **Implicit Error Handling:** Soon the data APIs will throw Javascript exceptions allowing for writing code with less explicit error
checking.

* **Additional Security Features:** The policy system has some evolutions in the works that will allow many additional controls, including
easy per verb policy changes.

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
