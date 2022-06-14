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

Generate some test data, 1000 elements

```console
curl -X POST localhost:8080/dev/fixture/1000
```
Generate the needle elements to be found

```console
curl -X POST localhost:8080/dev/needle
```



Benchmark the three versions:
* array: converts the whole thing to an array and then find inside the array
* base: iterates through the asynchronous iterator
* bench: just write the lambda
* inline-validate: perform inline validation of the email
* explicit-validate: perform explicit validation of the email on the result of the search

```console
ab -c -n 100 localhost:8080/dev/array
ab -c -n 100 localhost:8080/dev/base
ab -c -n 100 localhost:8080/dev/bench
ab -c -n 100 localhost:8080/dev/inline-validate
ab -c -n 100 localhost:8080/dev/explicit-validate
```
