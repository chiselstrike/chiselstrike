# ChiselStrike Developer's Guide

## Generating API documentation

The ChiselStrike code has API documentation written in RustDoc. To generate
HTML pages, run:

```bash
cargo doc --no-deps --document-private-items
```

## Testing with Postgres

**Step 1**: Install Postgres client.

On **macOS**:

```bash
brew install libpq && echo 'export PATH="/opt/homebrew/opt/libpq/bin:$PATH"' >> ~/.zshrc
```

Verify that the client is working by running:

```bash
psql --version
```

which should output:

```console
psql (PostgreSQL) 14.2
```

**Step 2**: Set up a Postgres database.

On **macOS**, install [Postgres.app](https://postgresapp.com) and start it up.

To verify that Postgres database is up and running, run:

```bash
psql
```

and you should see:

```
psql (14.2)
Type "help" for help.

penberg=#
```

**Step 3**: Run integration tests with Postgres:

```bash
cargo test -p cli --test integration_tests -- --database postgres
```
