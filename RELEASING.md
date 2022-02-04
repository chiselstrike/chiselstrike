# Releasing ChiselStrike

## Releasing binaries

**CentOS:** You need the following prerequisites:

```
yum install gcc git make
```

First, install Rust on your machine with [rustup](https://rustup.rs):

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then build the tarball with `scripts/build-tarball.sh` script, which generates a `chiselstrike-<version>-<target>.tar.gz` file for you.

**Mac/Intel:**

```
./scripts/build-tarball.sh -t x86_64-apple-darwin
```

**Mac/Apple Silicon (really arm):**

```
./scripts/build-tarball.sh -t aarch64-apple-darwin
```

**Linux**:

**Linux/Intel:**

**Build the tarball on a distribution with old enough glibc for compatibility between distributions. For example, build on CentOS 7 or Ubuntu 14.04.**

```
./scripts/build-tarball.sh -t x86_64-unknown-linux-gnu
```

## Releasing Docker image

Although we plan to make this better in the future, right now the way to release
is to push anything to a branch called "release". This will create two docker containers:

* chiseld
* toolset (contains chisel)

and push them to an ECR registry. There are two tags pushed: one with the git hash of the commit that
produced the image, and another with the special tag "latest"
