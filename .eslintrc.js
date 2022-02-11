module.exports = {
    root: true,
    parser: "@typescript-eslint/parser",
    parserOptions: {
        project: "./tsconfig.json"
    },
    plugins: [
        "@typescript-eslint"
    ],
    extends: [
        "eslint:recommended",
        "plugin:@typescript-eslint/recommended",
    ],
    ignorePatterns: [
        "/cli/examples/",
        "/examples/",
        "api/src/lib.deno_core.d.ts",
        "packages/chiselstrike-api/lib/chisel.d.ts",
        "packages/chiselstrike-api/lib/lib.deno_core.d.ts",
        "packages/chiselstrike-api/src/lib.deno_core.d.ts",
        "target/",
        "template/",
        "third_party/"
    ],
    rules: {
        "@typescript-eslint/switch-exhaustiveness-check": "error",
        "@typescript-eslint/triple-slash-reference": "off",
    },
};
