#!/bin/sh
#
# SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>
#
# Bump ChiselStrike project version across all the metadata files.
#
# Example usage:
#
#  bump-version.sh 0.1.2
#
# Updates the Cargo and npm metadata files with the version "0.1.2", makes a
# git commit, and creates a "v0.1.2" git tag.

if [ $# -eq 0 ]; then
  cat << EOF
usage: bump-version.sh [version]

Example usage:

  bump-version.sh 0.1.2
EOF
    exit 1
fi

version=$1

cargo set-version --workspace $1

cwd=$(pwd)

cd "$cwd/packages/chiselstrike" && npm version --no-git-tag-version $version
cd "$cwd/packages/chiselstrike-cli" && npm version --no-git-tag-version $version

git commit -a -m "ChiselStrike v$version"

git tag "v$version"
