#!/bin/sh
#
# SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>
#
# Publish ChiselStrike packages to npm registry.

cwd=$(pwd)

cargo build -p packages
cd "$cwd/packages/chiselstrike-api" && npm i && npm publish
cd "$cwd/packages/chiselstrike-cli" && npm i && npm publish
cd "$cwd/packages/chiselstrike-next-auth" && npm i && npm publish
cd "$cwd/packages/create-chiselstrike-app" && npm i && npm publish
