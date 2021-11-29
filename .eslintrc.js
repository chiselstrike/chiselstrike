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
    ignorePatterns: ["/examples/", "/cli/examples/"],
    rules: {
        "@typescript-eslint/switch-exhaustiveness-check": "error",
    },
};
