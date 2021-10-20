#!/bin/sh

set -e

$CHISELD -d sqlite://:memory: -m sqlite://:memory: &
PID=$!
sleep 1

set +e
sh -c "$2"
ret=$?
set -e

kill $PID
wait

exit $ret
