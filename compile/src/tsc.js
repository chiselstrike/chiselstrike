const readCache = {};
function compile(file, lib) {
    const options = {
        allowJs: true,
        noEmitOnError: true,
        noImplicitAny: true,
        declaration: true,
        outDir: "chisel://",
        rootDir: "/",
        target: ts.ScriptTarget.ESNext,
        module: ts.ModuleKind.ESNext,
        types: [],
    };

    const host = ts.createCompilerHostWorker(options, false, {});
    host.getNewLine = () => {
        return "\n";
    };
    host.directoryExists = (path) => {
        return Deno.core.opSync("dir_exists", path);
    };
    host.fileExists = (path) => {
        return Deno.core.opSync("file_exists", path);
    };
    host.getCurrentDirectory = () => {
        return Deno.core.opSync("get_cwd");
    };
    host.getDefaultLibLocation = () => {
        return "/default/lib/location";
    };
    host.getDefaultLibFileName = () => {
        return lib;
    };
    host.writeFile = (fileName, contents) => {
        Deno.core.opSync("write", fileName, contents);
    };
    host.resolveModuleNames = (moduleNames, containingFile) => {
        const ret = [];
        for (const name of moduleNames) {
            let fname = Deno.core.opSync("fetch", name, containingFile);
            ret.push({ resolvedFileName: fname });
        }
        return ret;
    };
    host.resolveTypeReferenceDirectives = (
        typeReferenceDirectiveNames,
        containingFile,
        redirectedReference,
        options,
    ) => {
        const ret = [];
        for (const name of typeReferenceDirectiveNames) {
            let fname = Deno.core.opSync(
                "fetch",
                name + ".d.ts",
                containingFile,
            );
            ret.push({ resolvedFileName: fname });
        }
        return ret;
    };
    host.readFile = (specifier) => {
        let v = readCache[specifier];
        if (v !== undefined) {
            return v;
        }
        v = Deno.core.opSync("read", specifier);
        readCache[specifier] = v;
        return v;
    };

    let program = ts.createProgram([file], options, host);
    let emitResult = program.emit();

    let allDiagnostics = ts
        .getPreEmitDiagnostics(program)
        .concat(emitResult.diagnostics);

    allDiagnostics = ts.sortAndDeduplicateDiagnostics(allDiagnostics);
    if (allDiagnostics.length != 0) {
        let diag = ts.formatDiagnosticsWithColorAndContext(
            allDiagnostics,
            host,
        );
        Deno.core.opSync("diagnostic", diag);
    }
    return !emitResult.emitSkipped;
}

compile("bootstrap.ts", "/default/lib/location/lib.esnext.d.ts");
