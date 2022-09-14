#!/bin/sh
# SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>
#
# A test for comparing `chiselc` output to the source file for testing that
# the TypeScript backend ("pretty printer") generates code that is
# semantically equivalent to the source file.

input="$1"
output_jsmin="$input.jsmin.ts"
output_chiselc="$input.chiselc.ts"

echo "Verifying $input compilation ..."
cat $input | jsmin > $output_jsmin
deno fmt $output_jsmin
chiselc $input | jsmin > $output_chiselc
deno fmt $output_chiselc
diff -u $output_jsmin $output_chiselc
