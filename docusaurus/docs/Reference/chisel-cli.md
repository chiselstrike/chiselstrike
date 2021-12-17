# Command-Line Reference

This document is the user manual for the ChiselStrike command line tool, `chisel`.

## `chisel apply`

The `chisel apply` command updates `chiseld` state as per a manifest file `Chisel.toml`, which has the following format:

```toml
types = ["types"]
endpoints = ["endpoints"]
policies = ["policies"]
```

If a `Chisel.toml` file does not exists, types are read from a `types` directory, endpoints from an `endpoints` directory, and policies from a `policies` directory.

## `chisel status`

The `chisel status` command queries a ChiselStrike server for its status.

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

## `chisel describe`

The `chisel describe` command displays the current state of the running ChiselStrike server: types, endpoints, and policies.
