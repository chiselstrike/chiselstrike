# ChiselStrike Command-Line Reference

This is the command-line reference for the ChiselStrike `chisel` [command-line interface](#cli) and the `chiseld` [server](#server).

## CLI

The command-line interface (CLI) is the main program you use to interact with ChiselStrike during development.

Overview of commands

* [`apply`](#chisel-apply) - apply state
* [`delete`](#chisel-delete) - delete state
* [`describe`](#chisel-describe) - describe state
* [`dev`](#chisel-dev) - start development server
* [`help`](#chisel-help) - print help
* [`init`](#chisel-init) - create a new project in current directory
* [`new`](#chisel-new) - create a new project
* [`restart`](#chisel-restart) - restart server
* [`start`](#chisel-start) - start server
* [`status`](#chisel-status) - show server status
* [`wait`](#chisel-wait) - wait for server to start

### `chisel apply`

Applies the contents of the current project to the ChiselStrike server.

By default, ChiselStrike files are organized as follows:

* `types` directory contains type definitions
* `endpoints` directory contains endpoint definitions
* `policies` directory contains policy definitions

The directory structure can also be changed via an optional [manifest file](#manifest-files).

**See also:**

* [`describe`](#chisel-describe)
* [`dev`](#chisel-dev)

### `chisel delete`

TODO

### `chisel describe`

The `chisel describe` command displays the current state of the running ChiselStrike server: types, endpoints, and policies.

### `chisel dev`

Start the ChiselStrike server in development mode. In this mode, the CLI watches for filesystem changes in the current project, and performs [`apply`](#chisel-apply) automatically.

**See also:**

* [`apply`](#chisel-apply)

### `chisel help [COMMAND]`

Prints a help message or the help of the given `COMMAND`.

### `chisel init`

Create a new ChiselStrike project in current directory.

**Example:**

```bash
$ chisel init
Created ChiselStrike project in /.../hello
```

**See also:**

* [`new`](#chisel-new)
* [`dev`](#chisel-apply)
* [`apply`](#chisel-dev)

### `chisel new [PATH]`

Create a new ChiselStrike project in `PATH` directory.

**Example:**

```bash
$ chisel new hello
Created ChiselStrike project in hello
```

**See also**:

* [`init`](#chisel-init)
* [`dev`](#chisel-dev)
* [`apply`](#chisel-dev)

### `chisel restart`

Restarts the ChiselStrike server.

**Example:**

```
$ chisel restart
Server restarted successfully.
```

### `chisel start`

Starts the ChiselStrike server.

### `chisel status`

Show status of the ChiselStrike server.

**Example:**

```bash
$ chisel status
Server status is OK
```

**See also:**

* [`wait`](#chisel-wait)

### `chisel wait`

Wait for the ChiselStrike server to start up. The `chisel wait` exits only when the server is up and running, or the command times out.

```bash
$ chisel wait
```

* [`status`](#chisel-status)

## Server

The `chiseld` program is the ChiselStrike server daemon. For development purposes, you don't need to interact with it.

#### `--api-listen-addr [ADDR]`

The API listen address of the server. This is the address that servers ChiselStrike endpoints.

#### `--data-db-uri [URI]`

The database URI to connect to.

#### `--executor-threads [COUNT]`

The number of executor threads the ChiselStrike server uses.

#### `--internal-routes-listen-addr [ADDR]`

The internal routes listen address of the server. This is the address that serves healthcheck for things like k8s.

#### `--metadata-db-uri [URI]`

The metadata database URI to connect to.

#### `--rpc-listen-addr [ADDR]`

The RPC listen address of the server. This is the address that the ChiselStrike CLI connects to to interact with the server.

## Manifest files

The CLI parses an optional manifest file `Chisel.toml`, which has the following format:

```toml
types = ["types"]
endpoints = ["endpoints"]
policies = ["policies"]
```
