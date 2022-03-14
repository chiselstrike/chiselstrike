// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

(function () {
    const host = {
        useCaseSensitiveFileNames() {
            return true;
        },
        getCanonicalFileName(name) {
            return name;
        },
        getSourceFile(
            fileName,
            languageVersion,
            onError,
            _shouldCreateNewSourceFile,
        ) {
            let text;
            try {
                text = host.readFile(fileName);
            } catch (e) {
                onError(e.message);
            }
            return text === undefined ? undefined : ts.createSourceFile(
                fileName,
                text,
                languageVersion,
                false,
            );
        },
        getNewLine() {
            return "\n";
        },
        directoryExists(path) {
            return Deno.core.opSync("dir_exists", path);
        },
        fileExists(path) {
            return Deno.core.opSync("file_exists", path);
        },
        getCurrentDirectory() {
            return Deno.core.opSync("get_cwd");
        },
        getDefaultLibLocation() {
            return "/default/lib/location";
        },
        getDefaultLibFileName() {
            return undefined;
        },
        writeFile(fileName, contents) {
            Deno.core.opSync("write", fileName, contents);
        },
        resolveModuleNames(moduleNames, containingFile) {
            const ret = [];
            for (const name of moduleNames) {
                const fname = Deno.core.opSync(
                    "fetch",
                    name,
                    containingFile,
                );
                // FIXME: Not every file is typescript. We say it is to
                // handle user libraries that don't end in .ts
                // (like @foo/bar). We should probably get the extension
                // from rust.
                ret.push({ resolvedFileName: fname, extension: ".ts" });
            }
            return ret;
        },
        resolveTypeReferenceDirectives(
            typeReferenceDirectiveNames,
            containingFile,
            _redirectedReference,
            _options,
        ) {
            const ret = [];
            for (const name of typeReferenceDirectiveNames) {
                const fname = Deno.core.opSync(
                    "fetch",
                    name,
                    containingFile,
                );
                ret.push({ resolvedFileName: fname });
            }
            return ret;
        },
        readFile(specifier) {
            let v = readCache[specifier];
            if (v !== undefined) {
                return v;
            }
            v = Deno.core.opSync("read", specifier);
            readCache[specifier] = v;
            return v;
        },
    };

    const readCache = {};
    function compileAux(file, lib, emitDeclarations) {
        // FIXME: This is probably not exactly what we want. Deno uses
        // deno.window. This is the subset of deno.window that is
        // compatible with lib.dom.d.ts + lib.dom.d.ts. It should probably
        // be the subset of deno that we want + our own chisel namespace.
        const defaultLibs = [
            "lib.deno.ns.d.ts",
            "lib.dom.asynciterable.d.ts",
            "lib.dom.iterable.d.ts",
            "lib.dom.d.ts",
            "lib.deno_console.d.ts",
            "lib.deno_broadcast_channel.d.ts",
            "lib.esnext.d.ts",
        ];
        if (lib !== undefined) {
            defaultLibs.push(lib);
        }

        const options = {
            allowJs: true,
            allowSyntheticDefaultImports: true,
            declaration: emitDeclarations,
            emitDecoratorMetadata: false,
            experimentalDecorators: true,
            isolatedModules: true,
            lib: defaultLibs,
            module: ts.ModuleKind.ESNext,
            noEmitOnError: true,
            noImplicitAny: true,
            outDir: "chisel://",
            removeComments: true,
            strictPropertyInitialization: false, // we don't support constructors, so don't be strict about this
            rootDir: "/",
            strict: true,
            target: ts.ScriptTarget.ESNext,
            types: [],
        };

        const program = ts.createProgram([file], options, host);
        const emitResult = program.emit();

        let allDiagnostics = ts
            .getPreEmitDiagnostics(program)
            .concat(emitResult.diagnostics);

        allDiagnostics = ts.sortAndDeduplicateDiagnostics(allDiagnostics);
        if (allDiagnostics.length != 0) {
            const diag = ts.formatDiagnosticsWithColorAndContext(
                allDiagnostics,
                host,
            );
            Deno.core.opSync("diagnostic", diag);
        }
        return !emitResult.emitSkipped;
    }

    function compile(file, lib, emitDeclarations) {
        try {
            return compileAux(file, lib, emitDeclarations);
        } catch (e) {
            Deno.core.opSync("diagnostic", e.stack + "\n");
            return false;
        }
    }

    // Add the deno libraries
    // FIXME: get this list from build.rs
    const libs = {
        "deno.broadcast_channel": "deno_broadcast_channel",
        "deno.console": "deno_console",
        "deno.core": "deno_core",
        "deno.crypto": "deno_crypto",
        "deno.fetch": "deno_fetch",
        "deno.net": "deno_net",
        "deno.ns": "deno.ns",
        "deno.shared_globals": "deno.shared_globals",
        "deno.url": "deno_url",
        "deno.web": "deno_web",
        "deno.webgpu": "deno_webgpu",
        "deno.websocket": "deno_websocket",
        "deno.webstorage": "deno_webstorage",
    };

    for (const k in libs) {
        v = libs[k];
        if (!ts.libs.includes(k)) {
            ts.libs.push(k);
            ts.libMap.set(k, `lib.${v}.d.ts`);
        }
    }

    compile("bootstrap.ts", undefined);

    globalThis.compile = compile;
})();
