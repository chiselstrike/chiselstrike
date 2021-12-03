# ChiselStrike Server Command Line Manual

This document is the user manual for the ChiselStrike server, `chiseld`.

## Options

### `--metadata-db-uri [URI]`

The `--metadata-db-uri` option specifies the URI of the database that is
used to store ChiselStrike server metadata such as type system
definition.

**Examples:**

Connect to a PostgreSQL database with username `postgres`, password
`password`, on host `localhost`, and database `chiseld`:

```
chiseld --metadata-db-uri postgres://postgres:password@localhost/chiseld
```

Connect to a file-backed SQLite database with the filename `chiseld.db`:

```
chiseld --metadata-db-uri sqlite://chiseld.db
```

Connect to an in-memory SQLite database:

```
chiseld --metadata-db-uri sqlite://:memory:
```


### Docker/Podman containers:


The docker/podman containers enable the same set of settables as the 
`chiseld` server (with some minor differences).

`API_PORT`  API server listen port [default: 8080]

`DATA_DB_URI` Data database URI [default: sqlite://chiseld-data.db?mode=rwc]

`EXECUTOR_THREADS` How many executor threads to create [default: 1]

`METADATA_DB_URI` Metadata database URI [default: sqlite://chiseld.db?mode=rwc]

`RPC_PORT` RPC server listen port [default: 50051]

`API_PORT` and `RPC_PORT` are exposed by the container.


**Example for overriding the ports:**

```bash
docker run -d -e API_PORT=8081 -e RPC_PORT=8082 -p 8081:8081 -p 8082:8082 chiseld:latest
```


