# Performance Benchmarks

## Getting Started

Build the release version for better performance

```console
cargo build --release
```
Start ChiselStrike server:

```console
cd backend && ../../target/release/chisel dev
```

Generate some test data:

```console
curl -X POST localhost:8080/dev/fixture
```

Benchmark the three versions:
* array: converts the whole thing to an array and then find inside the array
* base: iterates through the asynchronous iterator
* bench: just write the lambda

```console
ab -c -n 100 localhost:8080/dev/array
ab -c -n 100 localhost:8080/dev/base
ab -c -n 100 localhost:8080/dev/bench
```
