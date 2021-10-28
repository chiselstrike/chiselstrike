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

sleep 1

for i in {1..5}; do
  $CHISEL status
  if [ $? -eq 0 ]; then
    break
  fi
  sleep $i
done

set +e
sh -c "$2"
ret=$?
set -e

exit $ret
