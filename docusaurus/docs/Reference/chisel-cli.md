# Command-Line Reference

This is the command line reference for the ChiselStrike command line tool, `chisel`.

Overview of commands:

* [`apply`](#chisel-apply) - apply configuration
* [`delete`](#chisel-delete) - delete configuration
* [`describe`](#chisel-describe) - describe configuration
* [`dev`](#chisel-dev) - start development server
* [`help`](#chisel-help) - print help
* [`init`](#chisel-init) - initialize a new project
* [`restart`](#chisel-restart) - restart server
* [`status`](#chisel-status) - show server status
* [`wait`](#chisel-wait) - wait for server to start

## `chisel apply`

The `chisel apply` command updates `chiseld` state as per a manifest file `Chisel.toml`, which has the following format:

```toml
types = ["types"]
endpoints = ["endpoints"]
policies = ["policies"]
```

If a `Chisel.toml` file does not exists, types are read from a `types` directory, endpoints from an `endpoints` directory, and policies from a `policies` directory.

## `chisel delete`

TODO

## `chisel describe`

The `chisel describe` command displays the current state of the running ChiselStrike server: types, endpoints, and policies.

## `chisel dev`

TODO

## `chisel help`

TODO

## `chisel init`

TODO

## `chisel restart`

TODO

## `chisel status`

The `chisel status` command queries a ChiselStrike server for its status.
