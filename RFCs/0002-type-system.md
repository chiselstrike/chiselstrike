# RFC 2: Type System

This RFC describes the type system of the ChiselStrike platform.

## Built-in types

ChiselStrike has the following built-in types (same as GraphQL):

* `ID` is an opaque type that serializes to `String`.
* `String` is a sequence of Unicode points.
* `Int` is a signed 32-bit integer.
* `Float` is a signed double precision floating point.
* `Boolean` is a scalar type that represents `true` or `false` values.

TODO: `List`

TODO: Objects

TODO: Prisma type system

## Types

### Adding a new type

The user can add a new type to the type system with the ``chisel type add [TYPE NAME]`` command.

If the given type name already exists in the type system, the command fails.

### Renaming a type

The user can rename a type in the type system with the ``chisel type rename [OLD NAME] [NEW NAME]``.

When a type is renamed, a type alias with the old type name is added to avoid breaking existing endpoints.

If the given old type name does not exists in the type system, the command fails.

If the given new type name already exists in the type system, the command fails.

### Removing a type

The user can remove a type from the type system with the ``chisel type remove [TYPE NAME]``.

If the given type name does not exists in the type system, the command fails.

If the given type name is referred to by other types or functions, the command fails.

## Fields

### Adding a field to a type

The user can add a new field to a type with the ``chisel type add-field [TYPE NAME] [FIELD NAME] [FIELD TYPE]`` command.

If the given type name does not exists in the type system, the command fails.

If the given field name already exists in the type, the command fails.

If the given field type does not exist in the type system, the command fails.

### Renaming a field in a type

The user can rename a field of a type with the ``chisel type rename-field [TYPE NAME] [OLD FIELD NAME] [NEW FIELD NAME]`` command. 

If the given type name does not exists in the type system, the command fails.

If the given old field name does not exist in the type, the command fails.

If the given field name already exists in the type, the command fails.

### Removing a field from a type

The user can remove a field from a type with the ``chisel type remove-field [TYPE NAME] [FIELD NAME]`` command.

If the given type name does not exists in the type system, the command fails.

If the given field name does not exists in the type system, the command fails.

If the given field name is referred to by other types or functions, the command fails.

### Changing the type of a field

The user can change the type of a field in a type with the ``chisel type change-field [TYPE NAME] [FIELD NAME] [NEW TYPE]`` command.

If the given type name does not exists in the type system, the command fails.

If the given field name does not exists in the type system, the command fails.

If the given new field type is incompatible with the old type, the command fails.

## Advanced

### Importing types

The user can import types with the `chisel type import [TYPE DEFINITION]` command.

The type definition is either a GraphQL or Prisma schema definition, or a set of TypeScript classes.

The import command attempts to automatically discover the transformations needed to import the types to the type system.

For example, if a type exists in the type system, but not in the type definition, the import command attempts to remove the type.
Another example, if a type in the type definition has a field that is not in the type in the type system, the import command attempts to add a field to the type.

If the import command is unable to perform all transformations needed to import types to the type system, the command will fail.

### Extracting a type from another type

TODO

### Moving a field from a type to another type

TODO

## References

https://spec.graphql.org/draft/

https://github.com/ankane/strong_migrations
