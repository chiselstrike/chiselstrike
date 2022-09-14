// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import indent from 'https://cdn.skypack.dev/pin/indent-string@v5.0.0-VgKPSgi4hUX5NbF4n3aC/mode=imports,min/optimized/indent-string.js'

function foo(a: string): string {
    return indent(a + "foo\n", 4);
}
