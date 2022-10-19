#!/bin/sh
#
# SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>
#
# Bump ChiselStrike project version across all the metadata files.
#
# Prerequisites:
#
#  cargo install set-cargo-version
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
cargo update

cwd=$(pwd)

cd "$cwd/packages/chiselstrike-api" && npm version --no-git-tag-version $version && npm update
cd "$cwd/packages/chiselstrike-cli" && npm version --no-git-tag-version $version && npm update
cd "$cwd/packages/chiselstrike-next-auth" && npm version --no-git-tag-version $version && npm update
cd "$cwd/packages/create-chiselstrike-app" && npm version --no-git-tag-version $version && npm update

git commit -a -m "ChiselStrike v$version"

git tag "v$version"
