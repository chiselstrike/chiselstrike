# ChiselStrike Protocols Specification

## Control protocol

The control protocol manages types and endpoints, for example.
The wire protocol uses Protobuf, and is defined in the `proto/chisel.proto` file of the source tree.

### Messages

* `StatusRequest` message is sent by a client to query for the status of the server.
  The server responds with a `StatusResponse` message.
* `TypeDefinitionRequest` message is sent by a client to define a type in the server type system.
  The serve responds with a `TypeDefinitionResponse` message.
