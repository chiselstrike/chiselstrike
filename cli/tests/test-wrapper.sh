#!/bin/bash

set -e

export TEMPDIR=$(mktemp -d)

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
