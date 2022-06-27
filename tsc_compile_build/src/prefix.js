const module = {};
const process = { env: {}};
function require(name) {
    if (name == "typescript") {
        return ts;
    }
    return undefined;
}
