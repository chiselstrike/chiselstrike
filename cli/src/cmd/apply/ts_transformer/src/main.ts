import * as path from "path";
import * as tsm from "ts-morph";
import * as fs from "fs";

async function transformSources(projectDir: string, routeDir: string, rootPath: string) {
    const project = new tsm.Project({
        tsConfigFilePath: path.join(projectDir, "tsconfig.json"),
    });
    console.log(fs.readFileSync(path.join(projectDir, "tsconfig.json"), "utf-8"));

    project.addSourceFilesAtPaths([path.join(routeDir, "/**/*{.d.ts,.ts}")]);
    project.resolveSourceFileDependencies();
    for (const file of project.getSourceFiles()) {
        console.log(file.getFilePath());
    }

    const diagnostics = project.getPreEmitDiagnostics();
    for (const diag of diagnostics) {
        console.log(diag.getMessageText());
    }

    console.log(project.getAmbientModules());
    for (const module of project.getAmbientModules()) {
        console.log(module.getEscapedName());
    }

    // const srcFile = project.getSourceFiles()[0];
    // srcFile.forEachDescendant((node, traversal) => {
    //     switch (node.getKind()) {
    //         case ts.SyntaxKind.CallExpression: {
    //             modifyCallExpression(node as tsm.CallExpression);
    //             break;
    //         }
    //     }
    // });
    // await srcFile.save();
}

const args = process.argv.slice(2);
// TODO: Proper arguments parsing
transformSources(args[0], args[1], args[2]);
