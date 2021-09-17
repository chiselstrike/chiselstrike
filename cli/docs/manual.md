# ChiselStrike Command Line Manual

This document is the user manual for the ChiselStrike command line tool, `chisel`.

## Command Reference

### `chisel status`

The `chisel status` command queries a ChiselStrike server for its status.

### `chisel type define [FILENAME]`

The `chisel type define` command defines a type in the ChiselStrike type system.

Example file looks as follows:

```
type Person {
  first_name: String,
  last_name: String,
}
```
