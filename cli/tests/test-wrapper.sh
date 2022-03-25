#!/bin/bash

set -e

# Enables backtraces on panic and anyhow errors.
export RUST_BACKTRACE=1

export TEMPDIR=$(mktemp -d)

cwd=$(pwd)

export CHISEL_SECRET_LOCATION="file://$TEMPDIR/.env"

EXTENSION=`basename "$2" | cut -d'.' -f2`

cd $TEMPDIR
if [ "x$EXTENSION" == "xnode" ]; then
    npx $CREATE_APP ./
else
    $CHISEL init --no-examples
fi
cd $cwd

if [ "x$TEST_DATABASE" == "xpostgres" ]; then
    DATADB="datadb_$(echo $RANDOM | shasum | head -c 40)"

    psql "$DATABASE_URL_PREFIX" -c "CREATE DATABASE $DATADB"

    DATADB_URL="$DATABASE_URL_PREFIX/$DATADB"
else
    METADB_URL="sqlite://$TEMPDIR/chiseld.db?mode=rwc"
    DATADB_URL="sqlite://$TEMPDIR/chiseld-data.db?mode=rwc"
fi

$CHISELD --webui -m "$DATADB_URL" -d "$DATADB_URL" --api-listen-addr "$CHISELD_HOST" --internal-routes-listen-addr "$CHISELD_INTERNAL" --rpc-listen-addr $CHISELD_RPC_HOST &
PID=$!

function cleanup() {
    kill $PID
    wait
    rm -rf "$TEMPDIR"
    if [ "x$TEST_DATABASE" == "xpostgres" ]; then
        psql "$DATABASE_URL_PREFIX" -c "DROP DATABASE $DATADB"
    fi
}

trap cleanup EXIT

$CHISEL wait

set +e
sh -c "$2"
ret=$?
set -e

exit $ret
