# Index JSON Reference

The `index-json` target for `chiselc` emits predicate indexes in JSON format.

## Example

The following `filter(restriction)` call:
 
```typescript
Person.cursor().filter({ name: name, age: age });
```

outputs the following predicate indexes:

```json
[{
	"entity_type": "Person",
	"properties": ["name", "age"]
}]
```

The `chiselc` compiler makes no guesses on the access patterns of the predicate.
It is up to a runtime or a query profiler to decide wheter to create the index or not.
