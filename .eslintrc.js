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
    ignorePatterns: ["/examples/", "/cli/examples/", "third_party/", "template/", "packages/chiselstrike-api/src/lib.deno_core.d.ts",
                     "packages/chiselstrike-api/lib/lib.deno_core.d.ts", "api/src/lib.deno_core.d.ts", "target/",
                     "packages/chiselstrike-api/lib/chisel.d.ts",
                     "packages/create-chiselstrike-app/dist",
                     "tsc_compile/tests/"],
    rules: {
        "@typescript-eslint/switch-exhaustiveness-check": "error",
        "@typescript-eslint/triple-slash-reference": "off",
    },
};
