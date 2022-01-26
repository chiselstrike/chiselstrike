#!/bin/sh
#
# SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>
#
# Publish ChiselStrike packages to npm registry.

cwd=$(pwd)

cd "$cwd/packages/chiselstrike-api" && npm publish
cd "$cwd/packages/chiselstrike-cli" && npm publish
cd "$cwd/packages/chiselstrike-frontend" && npm publish
cd "$cwd/packages/create-chiselstrike-app" && npm publish
