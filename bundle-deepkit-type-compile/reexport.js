const tc = require("@deepkit/type-compiler");
const ts = require("typescript");
const DeclarationTransformer = tc.DeclarationTransformer;
const ReflectionTransformer = tc.ReflectionTransformer;
module.exports = {
    DeclarationTransformer,
    ReflectionTransformer,
    ts
};
