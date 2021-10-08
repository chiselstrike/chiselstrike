# Running integration tests

Integration tests run on Docker, so we can have an image that has all the dependencies that we may need,
including libraries and databases.

There are two Dockerfiles that are relevant for unit tests:

* `Dockerfile.test.baseimg`: contains the base image with the dependencies and databases, but not the actual tests. Because it is heavy and may install and configure many services, it is built
infrequently

* `Dockerfile.test`: uses the base image, and is built at every PR.

## Base image conventions

Because the base image is heavy, we don't want to rebuild it in every PR. The `Dockerfile.test` image mentions which image it wants based on a hash of the base image file contents.

```
ARG BASEIMG
FROM ghcr.io/chiselstrike/chiselstrike:$BASEIMG
```

If you make changes to the base image, a new base image will be built.

## Using the test docker container locally

To use the docker image, you must be logged in to the Github container registry. You will have
to create a github personal access token. [This link](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry) shows you how

To build the image:

```
docker build -f Dockerfile.test --build-arg BASEIMG=$(md5sum Dockerfile.test.baseimg | awk '{print $1}') -t integration ./ && docker run integration
```

The test image will run the contents of the `integration-tests.sh` script. They can be as complex as you want. Just remember to copy any files that you may need from the build
into the `Dockerfile.test` image with the `COPY` command.
