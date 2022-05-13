# Filter Properties Reference

The `filter-properties` target for `chiselc` emits a set of entity properties that are used in filtering.
The runtime can use this as a set of _candidate indexes_.

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
