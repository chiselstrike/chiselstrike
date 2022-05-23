# Performance Benchmarks

## Getting Started

Start ChiselStrike server:

```console
cd backend && ../../target/debug/chisel dev
```

Generate some test data:

```console
curl -X POST localhost:8080/dev/fixture
```

Benchmark:

```console
curl -d '{"name": "Alvin Wisoky", "email": "alwin@wisoky.me"}' http://localhost:8080/dev/users
httpstat localhost:8080/dev/find
```
