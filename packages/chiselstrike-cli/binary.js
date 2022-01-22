const { Binary } = require("binary-install");

const os = require("os");

const { version } = require("./package.json");

const getTarget = () => {
    const type = os.type();
    const arch = os.arch();

    if (type === "Linux" && arch == "x64") return "x86_64-unknown-linux-gnu";
    if (type === "Darwin" && arch === "x64") return "x86_64-apple-darwin";
    if (type === "Darwin" && arch === "arm64") return "aarch64-apple-darwin";

    throw new Error(`Unsupported platform: ${type} ${arch}`);
};

const getBinary = () => {
    const target = getTarget();
    const url =
        `https://downloads.chiselstrike.com/chiselstrike/beta/chiselstrike-v${version}-${target}.tar.gz`;
    const name = "chisel";
    return new Binary(name, url);
};

const install = () => {
    const binary = getBinary();
    binary.install();
};

const run = () => {
    const binary = getBinary();
    binary.run();
};

module.exports = {
    install,
    run,
};
