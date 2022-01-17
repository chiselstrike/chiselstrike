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
    ignorePatterns: ["/examples/", "/cli/examples/", "third_party/", "template/", "packages/chiselstrike/src/lib.deno_core.d.ts",
                     "packages/chiselstrike/lib/lib.deno_core.d.ts", "api/src/lib.deno_core.d.ts"],
    rules: {
        "@typescript-eslint/switch-exhaustiveness-check": "error",
        "@typescript-eslint/triple-slash-reference": "off",
    },
};
