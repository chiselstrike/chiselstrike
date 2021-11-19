#!/bin/bash

set -e

# Enables backtraces on panic and anyhow errors.
export RUST_BACKTRACE=1

export TEMPDIR=$(mktemp -d)
mkdir -p "$TEMPDIR/types" "$TEMPDIR/endpoints" "$TEMPDIR/policies"

$CHISELD -m "sqlite://$TEMPDIR/chiseld.db?mode=rwc" -d "sqlite://$TEMPDIR/chiseld-data.db?mode=rwc" &
PID=$!

function cleanup() {
    kill $PID
    wait
    rm -rf "$TEMPDIR"
}

trap cleanup EXIT

$CHISEL wait

set +e
sh -c "$2"
ret=$?
set -e

exit $ret
