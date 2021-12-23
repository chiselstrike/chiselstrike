---
sidebar_position: 3
---
# ChiselStrike's Data Access API

If you recall from the previous examples, data was stored by calling `Chisel.store`,
and read by just referring to the entity name (`Comment`), which is an async iterator
over all elements of the type `Comment`.

You can just iterate over all the elements, but if you prefer, the following functions
are provided. As a reminder, keep in mind that there isn't a 1:1 mapping between this and
queries. ChiselStrike is free to optimize this code.

* *findMany()*: Filter just the elements that match a certain column value

```typescript
Chisel.Comment.findMany({"content": something});
```

* *findOne()*: Return a single element that match a certain column value

```typescript
Chisel.Comment.findOne({"content": something});
```

* *select()*:  Restricts which columns to be added to the json object. Other properties are then
discarded.

```typescript
Chisel.Comment.select("content");
```

:::info Feedback Requested! We could use your help!
* Is your preference to open code your business logic and allow ChiselStrike to perform optimizations,
or do you feel more confident using this API?

* Which other functions would you like to see supported?
:::

:::warning
There are currently known issues of using ChiselStrike's API and its interactions with policies, that may lead to
crashes. If experimenting with policies, we recommend open coding your endpoints at this time.
:::
