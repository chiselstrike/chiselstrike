"use strict";
var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
var __generator = (this && this.__generator) || function (thisArg, body) {
    var _ = { label: 0, sent: function() { if (t[0] & 1) throw t[1]; return t[1]; }, trys: [], ops: [] }, f, y, t, g;
    return g = { next: verb(0), "throw": verb(1), "return": verb(2) }, typeof Symbol === "function" && (g[Symbol.iterator] = function() { return this; }), g;
    function verb(n) { return function (v) { return step([n, v]); }; }
    function step(op) {
        if (f) throw new TypeError("Generator is already executing.");
        while (_) try {
            if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
            if (y = 0, t) op = [op[0] & 2, t.value];
            switch (op[0]) {
                case 0: case 1: t = op; break;
                case 4: _.label++; return { value: op[1], done: false };
                case 5: _.label++; y = op[1]; op = [0]; continue;
                case 7: op = _.ops.pop(); _.trys.pop(); continue;
                default:
                    if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) { _ = 0; continue; }
                    if (op[0] === 3 && (!t || (op[1] > t[0] && op[1] < t[3]))) { _.label = op[1]; break; }
                    if (op[0] === 6 && _.label < t[1]) { _.label = t[1]; t = op; break; }
                    if (t && _.label < t[2]) { _.label = t[2]; _.ops.push(op); break; }
                    if (t[2]) _.ops.pop();
                    _.trys.pop(); continue;
            }
            op = body.call(thisArg, _);
        } catch (e) { op = [6, e]; y = 0; } finally { f = t = 0; }
        if (op[0] & 5) throw op[1]; return { value: op[0] ? op[1] : void 0, done: true };
    }
};
exports.__esModule = true;
var path = require("path");
var tsm = require("ts-morph");
var fs = require("fs");
function transformSources(projectDir, routeDir, rootPath) {
    return __awaiter(this, void 0, void 0, function () {
        var project, _i, _a, file_1, diagnostics, _b, diagnostics_1, diag, _c, _d, module_1;
        return __generator(this, function (_e) {
            project = new tsm.Project({
                tsConfigFilePath: path.join(projectDir, "tsconfig.json")
            });
            console.log(fs.readFileSync(path.join(projectDir, "tsconfig.json"), "utf-8"));
            project.addSourceFilesAtPaths([path.join(routeDir, "/**/*{.d.ts,.ts}")]);
            project.resolveSourceFileDependencies();
            for (_i = 0, _a = project.getSourceFiles(); _i < _a.length; _i++) {
                file_1 = _a[_i];
                console.log(file_1.getFilePath());
            }
            diagnostics = project.getPreEmitDiagnostics();
            for (_b = 0, diagnostics_1 = diagnostics; _b < diagnostics_1.length; _b++) {
                diag = diagnostics_1[_b];
                console.log(diag.getMessageText());
            }
            console.log(project.getAmbientModules());
            for (_c = 0, _d = project.getAmbientModules(); _c < _d.length; _c++) {
                module_1 = _d[_c];
                console.log(module_1.getEscapedName());
            }
            return [2 /*return*/];
        });
    });
}
var args = process.argv.slice(2);
// TODO: Proper arguments parsing
transformSources(args[0], args[1], args[2]);
