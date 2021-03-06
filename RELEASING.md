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

You also need to install Node.js:

```
curl -fsSL https://rpm.nodesource.com/setup_16.x | bash -
yum install nodejs
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
