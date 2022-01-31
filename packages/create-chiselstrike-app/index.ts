#!/usr/bin/env node

import Commander from "commander";
import chalk from "chalk";
import fs from "fs";
import packageJson from "./package.json";
import path from "path";
import spawn from "cross-spawn";

function run(projectDirectory: string) {
    projectDirectory = path.resolve(projectDirectory);
    if (fs.existsSync(projectDirectory)) {
        console.log(
            `Cannot create ChiselStrike project: directory ${
                chalk.red(projectDirectory)
            } already exists.`,
        );
        process.exit(1);
    }
    console.log(
        `Creating a new ChiselStrike project in ${
            chalk.green(projectDirectory)
        } ...`,
    );
    fs.mkdirSync(projectDirectory);
    fs.mkdirSync(path.join(projectDirectory, ".vscode"));
    fs.mkdirSync(path.join(projectDirectory, "endpoints"));
    fs.mkdirSync(path.join(projectDirectory, "models"));
    fs.mkdirSync(path.join(projectDirectory, "policies"));
    const rootFiles = [
        "Chisel.toml",
        "package.json",
        "tsconfig.json",
    ];
    for (const f of rootFiles) {
        fs.copyFileSync(
            path.join(__dirname, "template", f),
            path.join(projectDirectory, f),
        );
    }
    fs.appendFileSync(
        path.join(projectDirectory, "Chisel.toml"),
        'modules = "node"\n',
    );

    fs.copyFileSync(
        path.join(__dirname, "template", "settings.json"),
        path.join(projectDirectory, ".vscode", "settings.json"),
    );
    fs.copyFileSync(
        path.join(__dirname, "template", "hello.ts"),
        path.join(projectDirectory, "endpoints", "hello.ts"),
    );
    fs.copyFileSync(
        path.join(__dirname, "template", "hello.ts"),
        path.join(projectDirectory, "endpoints", "hello.ts"),
    );
    console.log("Installing packages. This might take a couple of minutes.");
    process.chdir(projectDirectory);
    spawn("npm", ["install"], {
        stdio: "inherit",
    });
}

const _program = new Commander.Command(packageJson.name)
    .version(packageJson.version)
    .arguments("<project-directory>")
    .action((projectDirectory) => {
        run(projectDirectory);
    })
    .parse(process.argv);
