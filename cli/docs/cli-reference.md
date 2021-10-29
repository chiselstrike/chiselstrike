# Command-Line Reference

This document is the user manual for the ChiselStrike command line tool, `chisel`.

## `chisel status`

The `chisel status` command queries a ChiselStrike server for its status.

## `chisel type import [FILENAME]`

The `chisel type import` command imports types from a definition file to the ChiselStrike type system.

The definition file is in GraphQL schema definition format. Example file looks as follows:

```
type Person {
  first_name: String,
  last_name: String,
}
```

## `chisel end-point create [PATH] [FILENAME]`

Creates a new endpoint at the given path that executes the code from
the given file.

Example endpoint code looks as follows:

```javascript
// hello.js
async function chisel(req) {
    const response = "hello, world";
    return new Response(response, {
        status: 200,
        headers: [],
    });
}
```

You can create an ChiselStrike endpoint with the following command:

```
chisel end-point create hello hello.js
```

## `chisel type export`

The `chisel type export` command exports the whole type system as TypeScript classes.
