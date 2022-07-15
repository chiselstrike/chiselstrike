# Query Expression Reference

## Example

For example, the following TypeScript code:

```typescript
const people = Person.cursor().filter(person => {
    return person.name != "Glauber" || person.age > 100;
});
```

compiles to the following:

```typescript
BlogPost.cursor().filter({
  exprType: "Binary",
  left: {
    exprType: "Property",
    object: { exprType: "Identifier", value: "post" },
    property: { exprType: "Identifier", value: "name" },
  },
  op: "=",
  right: { exprType: "Value", value: "Glauber" },
});
```

## Filtering

The `ChiselCursor.filter(<predicate>)` method call is transformed into a `__filter(<predicate>, <expression>)` method call where `<expression>` is an object that represents the query expression.

## Query Expressions

A query expression is a JavaScript object. Each object has a `exprType` property, which describes the type of the expression.

### Binary Expression

The `exprType` of a binary expression is a `Binary`.
A binary expression has three additional properties: `left` and `right`, which represent the left and right hand side of the binary expression, and an `op`, which represents the binary operator.

### Identifier

The `exprType` of an identifier expression is `Identifier`.

An identifier expression has a property `ident`, which is a string representing the identifier symbol.

### Value

The `exprType` of a value is `Value`.
A value has a `value` property, which can be a `string` or a `number`.

### Parameter

The `exprType` of a parameter is `Parameter`.
A parameter has a `position` property, which is a number representing the position of the parameter.

### Property access

The `exprType` of an a property access expression is a `Property`.
An property access expression has `object` and `property` properties, which are expressions representing the object and the property of that object being accessed.
