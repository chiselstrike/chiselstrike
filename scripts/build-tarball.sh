#!/bin/bash
#
# Tarball build script.
#
# SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
#
# This scripts builds a tarball of ChiselStrike for distribution. You need to
# specify the target (i.e. operating system and machine architecture) you are
# building for with the `-t TARGET` command line option. Please see `rustc
# --print target-list` for a list of available targets.

program="chiselstrike"

files=(
  chisel
  chiseld
)

while getopts "t:" option
do
  case $option in
    t)
      target="$OPTARG"
      ;;
  esac
done

version=$(git describe --tags 2> /dev/null || git rev-parse --short HEAD)
rustup target add "$target"
# FIXME: Use default job-level parallelism when the "bad archive" problem discussed in https://github.com/chiselstrike/chiselstrike/issues/777 is resolved.
cargo build --release --target "$target" -j1
mkdir -p "builds/$program-$target"
for file in ${files[@]}
do
  cp "target/$target/release/$file" "builds/$program-$target"
done
tar -C builds -czvf "$program-$version-$target.tar.gz" "$program-$target/"
