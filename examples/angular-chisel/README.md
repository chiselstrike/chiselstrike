# AngularChisel

This is an Angular example application using ChiselStrike. It provides a simple interface to upload images which will then be retrieved from ChiselStrike and shown (5 at a time, random selection).

## Getting Started

This guide expects you to be in the `examples/angular-chisel` folder. 

### Then, start up the ChiselStrike server:

```bash
chiseld
```

### Define types:

```bash
chisel type import graphql/types.graphql
```

### Define endpoints:

```
chisel end-point create api/import_images endpoints/import_images.js
chisel end-point create api/get_random_images endpoints/get_random_images.js
```

### Finally, to run the angular server:

Run `ng serve` for a dev server. If it crashes, you might need to do `npm update`. Then navigate to `http://localhost:4200/`. The app will automatically reload if you change any of the source files. 
