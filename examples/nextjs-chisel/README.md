This is a Next.js example application using ChiselStrike.

## Getting Started

First, start up the application server:

```bash
npm run dev
```

Then, start up ChiselStrike server:

```bash
chiseld
```

Define types:

```bash
chisel type import examples/nextjs-chisel/types.graphql
```

Define endpoint:

```
chisel end-point create posts examples/nextjs-chisel/endpoint.js
```

TODO: Insert some data to database.

Then go to localhost:3000.
