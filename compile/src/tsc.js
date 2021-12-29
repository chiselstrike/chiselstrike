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
        return undefined;
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
    host.resolveTypeReferenceDirectives = (typeReferenceDirectiveNames, containingFile, redirectedReference, options) => {
        const ret = [];
        for (const name of typeReferenceDirectiveNames) {
            let fname = Deno.core.opSync("fetch", name + ".d.ts", containingFile);
            ret.push({ resolvedFileName: fname });
        }
        return ret;
    };
    host.readFile = (specifier) => {
        return Deno.core.opSync("read", specifier);
    };

    let program = ts.createProgram([file], options, host);
    let emitResult = program.emit();

    let allDiagnostics = ts
        .getPreEmitDiagnostics(program)
        .concat(emitResult.diagnostics);

    for (const diagnostic of allDiagnostics) {
        if (diagnostic.file) {
            let { line, character } = ts.getLineAndCharacterOfPosition(
                diagnostic.file,
                diagnostic.start,
            );
            let message = ts.flattenDiagnosticMessageText(
                diagnostic.messageText,
                "\n",
            );
            Deno.core.opSync(
                "diagnostic",
                `${diagnostic.file.fileName} (${line + 1},${
                    character + 1
                }): ${message}`,
            );
        } else {
            Deno.core.opSync(
                "diagnostic",
                ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
            );
        }
    }
    return !emitResult.emitSkipped;
}
