# {{projectName}}

This project is a starter for a ChiselStrike application!

## Getting Started

To get going, run:

```console
npm run dev
```

Which starts a local development server of the example code.

For more help in getting started with ChiselStrike development, check out the
[online documentation](https://docs.chiselstrike.com).

## Docker Support

To build a Docker image of your application, type:

```console
docker build --tag {{projectName}} .
```

You can then start a container using the image with:

```
docker run --name={{projectName}} --network=host {{projectName}}
```

and access the endpoints:

```
curl localhost:8080/dev/hello
```
