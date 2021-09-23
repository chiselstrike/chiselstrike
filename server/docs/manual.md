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
