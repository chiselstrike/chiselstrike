#!/usr/bin/env node

import Handlebars from "handlebars";
import Commander from "commander";
import chalk from "chalk";
import fs from "fs";
import os from "os";
import packageJson from "./package.json";
import path from "path";
import spawn from "cross-spawn";

function isDirEmpty(dirname: string) {
    return fs.readdirSync(dirname).length === 0;
}

function run(
    projectDirectory: string,
    chiselApiVersion: string,
    chiselCliVersion: string,
    install: boolean,
    rewrite: boolean,
) {
    const projectName = path.basename(projectDirectory);
    projectDirectory = path.resolve(projectDirectory);
    if (fs.existsSync(projectDirectory)) {
        if (!rewrite && !isDirEmpty(projectDirectory)) {
            console.log(
                `Cannot create ChiselStrike project: directory ${
                    chalk.red(
                        projectDirectory,
                    )
                } already exists.`,
            );
            process.exit(1);
        }
    } else {
        fs.mkdirSync(projectDirectory);
    }
    console.log(
        `Creating a new ChiselStrike project in ${
            chalk.green(projectDirectory)
        } ...`,
    );

    const routesPath = path.join(projectDirectory, "routes");
    const eventsPath = path.join(projectDirectory, "events");
    const modelsPath = path.join(projectDirectory, "models");
    const policiesPath = path.join(projectDirectory, "policies");

    function mkdirpSync(path: string) {
        fs.mkdirSync(path, { recursive: true });
    }

    function touchSync(path: string) {
        fs.closeSync(fs.openSync(path, "w"));
    }

    mkdirpSync(path.join(projectDirectory, ".vscode"));
    mkdirpSync(routesPath);
    touchSync(path.join(routesPath, ".gitkeep"));
    mkdirpSync(eventsPath);
    touchSync(path.join(eventsPath, ".gitkeep"));
    mkdirpSync(modelsPath);
    touchSync(path.join(modelsPath, ".gitkeep"));
    mkdirpSync(policiesPath);
    touchSync(path.join(policiesPath, ".gitkeep"));
    const rootFiles = [
        "Chisel.toml",
        "Dockerfile",
        "package.json",
        "tsconfig.json",
    ];
    for (const f of rootFiles) {
        const source = fs.readFileSync(
            path.join(__dirname, "template", f),
            "utf8",
        );
        const template = Handlebars.compile(source);
        fs.writeFileSync(
            path.join(projectDirectory, f),
            template({ projectName, chiselApiVersion, chiselCliVersion }),
        );
    }
    fs.appendFileSync(
        path.join(projectDirectory, "Chisel.toml"),
        'modules = "node"\n',
    );

    const readmeSrc = fs.readFileSync(
        path.join(__dirname, "template", "README-template.md"),
        "utf8",
    );
    const readme = Handlebars.compile(readmeSrc);
    fs.writeFileSync(
        path.join(projectDirectory, "README.md"),
        readme({ projectName }),
    );
    fs.copyFileSync(
        path.join(__dirname, "template", "settings.json"),
        path.join(projectDirectory, ".vscode", "settings.json"),
    );
    fs.copyFileSync(
        path.join(__dirname, "template", "hello.ts"),
        path.join(projectDirectory, "routes", "hello.ts"),
    );
    fs.copyFileSync(
        path.join(__dirname, "template", "gitignore"),
        path.join(projectDirectory, "", ".gitignore"),
    );
    fs.copyFileSync(
        path.join(__dirname, "template", "dockerignore"),
        path.join(projectDirectory, "", ".dockerignore"),
    );

    if (install) {
        console.log(
            "Installing packages. This might take a couple of minutes.",
        );
        process.chdir(projectDirectory);
        spawn("npm", ["install"], {
            stdio: "inherit",
        });
    }
}

if (os.type() == "Windows_NT") {
    console.log(chalk.red("Error: Failed to create a ChiselStrike project."));
    console.log("");
    console.log(
        "ChiselStrike is currently supported on Windows through Windows Subsystem for Linux (WSL).",
    );
    console.log("");
    console.log(
        "Please create your project in an ext4 filesystem (like the $HOME folder) to support hot reloading of routes.",
    );
    console.log("");
    console.log(
        "For more information, see the documentation at: https://cs.docs.chiselstrike.com",
    );
    process.exit(1);
}

const _program = new Commander.Command(packageJson.name)
    .version(packageJson.version)
    .arguments("<project-directory>")
    .option(
        "-a, --chisel-api-version <version>",
        "ChiselStrike API version to use.",
        packageJson.version,
    )
    .option(
        "-c, --chisel-cli-version <version>",
        "ChiselStrike CLI version to use.",
        packageJson.version,
    )
    .option("--no-install", "Do not install dependencies")
    .option("--rewrite", "Rewrite an existing directory")
    .action((projectDirectory, options) => {
        run(
            projectDirectory,
            options.chiselApiVersion,
            options.chiselCliVersion,
            !!options.install,
            !!options.rewrite,
        );
    })
    .parse(process.argv);
