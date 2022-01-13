---
sidebar_position: 3
---
# ChiselStrike's Data Access API

If you recall from the previous examples, when we defined our `BlogComment` class, we required it to extend
`ChiselEntity`. That makes a couple of methods available to your class, one of them we already used: `cursor()`.

The methods in `ChiselEntity` all return a `ChiselCursor`. This is a lazy
iterator so you can compose them at will. An actual database query is only
generated when it is really needed: keep in mind that there isn't a 1:1 mapping
between this and queries. ChiselStrike is free to optimize this code.

The following methods are also available as part of `ChiselEntity`:

* *findMany()*: Filter just the elements that match a certain column value

```typescript
BlogComment.findMany({"content": something});
```

* *findOne()*: Return a single element that match a certain column value

```typescript
BlogComment.findOne({"content": something});
```

* *select()*:  Restricts which columns to be added to the json object. Other properties are then
discarded.

```typescript
BlogComment.select("content");
```

* *take(n: number)*: returns the first n elements, discarding the rest
discarded.

```typescript
BlogComment.take(1);
```

:::info Feedback Requested! We could use your help!
* Is your preference to open code your business logic and allow ChiselStrike to perform optimizations,
or do you feel more confident using this API?

* Which other functions would you like to see supported?
:::
