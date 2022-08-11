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
        "/cli/examples",
        "/third_party",
        "/packages/third_party",
        "/target",
        "/packages/chiselstrike-api/lib",
        "/packages/create-chiselstrike-app/dist",
        "/tsc_compile/tests",
    ],
    rules: {
        "@typescript-eslint/switch-exhaustiveness-check": "error",
        "@typescript-eslint/triple-slash-reference": "off",
    },
};
