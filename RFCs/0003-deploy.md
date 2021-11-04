# What is chiseld's internal state?

It's the collective definition of types, endpoints, and policies
currently known to chiseld.

# How does chiseld set its internal state?

A client runs `chisel apply` from a directory with the expected
structure (see #187).  This overwrites the prior state, though not
necessarily the content of any databases.  After `chisel apply`
succeeds, subsequent API requests to chiseld reflect the new internal
state.  This is easily done with endpoints and policies: the new state
simply overwrites the old.  Types are a bit different, though, because
they impact how user's data is stored and accessed.  Here it is not
enough for the new state to overwrite the old; existing user's data
may have to change, too.  In many cases, it's obvious how to bring the
user's data in sync with the new state; for instance, if we add a new
field with a default value, existing rows can be extended.

# What's an example of a valid type-system change for which it isn't obvious how to update the prior user's data?

Um... I'm actually not sure.  All the user-data conflict examples I
can think of seem like invalid type-system changes.

# How do multiple chiseld machines get to the same internal state?

We invoke `chisel apply` on each of them from the same set of files.
Note that different machines will reach the new state at different
times; for a brief while, there can be old-state and new-state (or
even transitioning, unresponsive) machines in the fleet.

# Is mixed-state fleet a problem?

Sure, but this is a long-standing problem in the modern web
architecture, and most apps live with it.

# Would persisting chiseld's internal state in a distributed database avoid a mixed-state fleet?

I don't think so.  Even if the database changed contents atomically
and reliably (belying the CAP theorem), independent chiselds can't
ensure they all query it _after_ the change to update their internal
state.  Without coordinating among themselves, their read queries will
race with the write query that updates the database.

# Would implementing Paxos in chiseld avoid a mixed-state fleet?

I don't think so.  Besides the CAP theorem, excluding frontend
machines from such coordination can still cause a type (or endpoint)
mismatch between a frontend instance and a chiseld.

# How do we canary a change to production with many chiseld machines?

We make each chiseld machine belong to a concentric circle (eg, 5%,
10%, 25%, 50%, 80%, 100% of the fleet).  When we need to roll out a
new state to production, we first make the 5% circle update and
monitor it for a while.  If there's a problem, we roll back those
machines to the old state.  Otherwise, we repeat the same for each
successive circle until we hit 100%.  We also partition the frontend
fleet and disallow inter-circle connections between the frontend and
the backend.  (Note that this still allows a mixed-state circle, but
hopefully just briefly.)

# What about non-production chiseld machines?

If there is a staging or testing fleet, we invoke `chisel apply` on
each of its machines from the same set of files before testing.  A
developer can run chiseld locally, invoking `chisel apply` on it from
their local files as a part of their framework's build step.
