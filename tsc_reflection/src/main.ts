import { assert, assertEquals } from "https://deno.land/std@0.173.0/testing/asserts.ts";
import * as path from "https://deno.land/std@0.173.0/path/mod.ts";
import * as tsm from "https://deno.land/x/ts_morph@17.0.1/mod.ts";

import { ReflectionType, getTypeReflection } from "./reflection.ts";

async function transformSources(projectDir: string, routesDir: string, mode: "node" | "deno") {
    if (mode !== "node") {
        throw new Error("Only Node mode is supported");
    }
    // const resolutionHost = mode === "deno" ? tsm.ResolutionHosts.deno : undefined;
    const project = new tsm.Project({
        tsConfigFilePath: path.join(projectDir, "tsconfig.json"),
    });

    project.addSourceFilesAtPaths([path.join(routesDir, "/**/*{.d.ts,.ts}")]);
    project.resolveSourceFileDependencies();

    const diagnostics = project.getPreEmitDiagnostics();
    for (const diag of diagnostics) {
        console.log(diag.getMessageText());
    }

    const tc = project.getTypeChecker();
    for (const srcFile of project.getSourceFiles()) {
        if (!srcFile.getFilePath().startsWith(routesDir)) {
            continue;
        }
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
        if (propertyAccess.getName() !== "post") {
            return;
        }
        const parent = propertyAccess.getParent();
        if (parent === undefined) {
            return;
        }
        const parentSymbol = parent.getType().getSymbol();
        if (parentSymbol === undefined || parentSymbol.getName() !== "RouteMap") {
            return;
        }

        const reflection = analyzeHandlerTypeArguments(tc, callExpr);
        if (reflection !== undefined) {
            // TODO: This is a little bit hacky, but ReflectionType
            // contains only objects and strings so it should work well.
            callExpr.addArgument(JSON.stringify(reflection));
        }
    }
}

type HandlerReflection = {
    request: {
        query: Record<string, ReflectionType>;
        jsonBody?: ReflectionType;
    };
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
        const requestParamSymbol = params[0];

        const requestArg = requestParamSymbol.getValueDeclarationOrThrow();
        const requestType = requestArg.getType();
        const requestTypeSymbol = requestType.getSymbol();
        if (requestTypeSymbol !== undefined && requestTypeSymbol.getName() === "ChiselRequest") {
            const requestParameter = requestParamSymbol
                .getValueDeclarationOrThrow()
                .asKindOrThrow(tsm.SyntaxKind.Parameter);
            const parameterChildren = requestParameter.getChildren();

            // Situation where there is parameter without type
            // (req) => {...}
            if (parameterChildren.length !== 3) {
                return undefined;
            }

            const typeNode = parameterChildren[2].asKindOrThrow(tsm.SyntaxKind.TypeReference);
            const typeArgs = typeNode.getTypeArguments();

            // If there are no type arguments, we don't reflect for now.
            // (req: ChiselRequest) => {...}
            if (typeArgs.length === 0) {
                return undefined;
            }

            const queryType = typeArgs[0].getType();
            const queryReflection = getTypeReflection(tc, queryType);
            if (queryReflection.name !== "namedObject" && queryReflection.name !== "anonymousObject") {
                throw new Error(
                    `got an an unexpected reflection type for TypedQuery: '${queryReflection.name}'`
                );
            }

            // There could be just QueryType specified.
            // (req: ChiselRequest<Query>) => {...} vs (req: ChiselRequest<Query, Body>) => {...}
            let bodyReflection;
            if (typeArgs.length >= 2) {
                const bodyType = typeArgs[1].getType();
                bodyReflection = getTypeReflection(tc, bodyType);
            }

            return {
                request: {
                    query: queryReflection.fields,
                    jsonBody: bodyReflection,
                },
            };
        }
    } else {
        // TODO
        // throw new Error("Not implemented");
    }
}

transformSources(Deno.args[0], Deno.args[1], Deno.args[2] as "deno" | "node");
