import * as path from "path";
import * as tsm from "ts-morph";
import * as fs from "fs";

import { assert, assertEquals } from "./utils";
import { ReflectionType, getTypeReflection } from "./reflection";

async function transformSources(projectDir: string) {
    const project = new tsm.Project({
        tsConfigFilePath: path.join(projectDir, "tsconfig.json"),
    });
    console.log(fs.readFileSync(path.join(projectDir, "tsconfig.json"), "utf-8"));
    const routesDir = path.join(projectDir, "routes");

    project.addSourceFilesAtPaths([path.join(routesDir, "/**/*{.d.ts,.ts}")]);
    project.resolveSourceFileDependencies();

    const diagnostics = project.getPreEmitDiagnostics();
    for (const diag of diagnostics) {
        console.log(diag.getMessageText());
    }

    console.log(project.getAmbientModules());
    for (const module of project.getAmbientModules()) {
        console.log(module.getEscapedName());
    }

    const tc = project.getTypeChecker();
    tc.getApparentType;
    for (const srcFile of project.getSourceFiles()) {
        if (!srcFile.getFilePath().startsWith(routesDir)) {
            continue;
        }
        console.log(srcFile.getFilePath());
        await processRouteFile(tc, srcFile);
    }
}

async function processRouteFile(tc: tsm.TypeChecker, srcFile: tsm.SourceFile) {
    srcFile.forEachDescendant((node, traversal) => {
        const callExpr = node.asKind(tsm.SyntaxKind.CallExpression);
        if (callExpr !== undefined) {
            modifyCallExpression(tc, node as tsm.CallExpression);
        }
    });
    await srcFile.save();
}

function modifyCallExpression(tc: tsm.TypeChecker, callExpr: tsm.CallExpression) {
    const propertyAccess = callExpr.getExpression().asKind(tsm.SyntaxKind.PropertyAccessExpression);
    if (propertyAccess !== undefined) {
        if (propertyAccess.getName() === "post") {
            const reflection = analyzeHandlerTypeArguments(tc, callExpr);
            if (reflection !== undefined) {
                // TODO: This is a little bit hacky, but ReflectionType
                // contains only objects and strings so it should work well.
                callExpr.addArgument(JSON.stringify(reflection));
            }
        }
    }
}

type HandlerReflection = {
    queryParams: Record<string, ReflectionType>;
    jsonBody: ReflectionType;
};

function analyzeHandlerTypeArguments(
    tc: tsm.TypeChecker,
    callExpr: tsm.CallExpression
): HandlerReflection | undefined {
    const args = callExpr.getArguments();
    assertEquals(args.length, 2, "Unexpected number of call arguments");
    const handlerArg = args[1];

    const arrowHandler = handlerArg.asKind(tsm.SyntaxKind.ArrowFunction);
    if (arrowHandler !== undefined) {
        const callSignature = arrowHandler.getSignature();
        const params = callSignature.getParameters();
        assertEquals(params.length, 1, "Endpoint handler must have only one argument");

        const requestArg = params[0].getValueDeclarationOrThrow();
        const requestType = tc.getTypeAtLocation(requestArg);
        const requestTypeSymbol = requestType.getSymbol();
        if (requestTypeSymbol !== undefined && requestTypeSymbol.getName() === "JsonRequest") {
            const typeArgs = requestType.getTypeArguments();
            assertEquals(
                typeArgs.length,
                2,
                "JsonRequest must have both QueryParams and JsonBody type arguments"
            );

            const queryType = typeArgs[0];
            const bodyType = typeArgs[1];
            const queryReflection = getTypeReflection(tc, queryType);
            if (queryReflection.name === "namedObject" || queryReflection.name === "anonymousObject") {
                return {
                    queryParams: queryReflection.fields as Record<string, ReflectionType>,
                    jsonBody: getTypeReflection(tc, bodyType),
                };
            } else {
                throw new Error(
                    `got an an unexpected reflection type for QueryParams: '${queryReflection.name}'`
                );
            }
        }

        // assertEquals(
        //     callSignatures.length,
        //     1,
        //     "Unexpected number of call signatures of Handler passed to RouteMap.route"
        // );
        // const callSignature = callSignatures[0];
        // const returnType = this.tc.getReturnTypeOfSignature(callSignature);
        // const simplifiedReturnType = typeToJsonSerializable(this.tc, returnType);
        // console.log(simplifiedReturnType);

        // const params = callSignature.parameters;
        // assertEquals(
        //     params.length,
        //     1,
        //     "Unexpected number of call parameters of Handler passed to RouteMap.route"
        // );
        // const queryParams = params[0].valueDeclaration;
        // assert(queryParams !== undefined, "QueryParams argument is missing a declaration");
        // const queryParamsType = this.tc.getTypeAtLocation(queryParams!);
        // const queryParamsSimplified = typeToJsonSerializable(this.tc, queryParamsType);
        // console.log(queryParamsSimplified);
        // return {
        //     paramsTypeId: this.registerUsedType(queryParamsSimplified),
        //     returnTypeId: this.registerUsedType(simplifiedReturnType),
        // };
    } else {
        // TODO
        // throw new Error("Not implemented");
    }
}

const args = process.argv.slice(2);
// TODO: Proper arguments parsing
transformSources(args[0]);
