"use strict";

class TestContext {
    constructor() {
        this.closeOnClose = [];
    }

    close() {
        for (const otherRes of this.closeOnClose) {
            otherRes.close();
        }
    }
}

class ProbeTestContext extends TestContext {
    constructor() {
        super();
        this.children = [];
    }

    async case(name, _callback) {
        this.children.push({type: "case", name});
    }

    async context(name, callback) {
        const childCtx = new ProbeTestContext();
        await callback(childCtx);
        this.children.push({type: "context", name, children: childCtx.children});
    }
}

class SelectTestContext extends TestContext {
    constructor(selectPath, children) {
        super();
        this.selectPath = selectPath;
        this.childIdx = 0;
        this.children = children;
    }

    async case(name, callback) {
        await this.#child("case", name, callback, (child) => new TestContext());
    }

    async context(name, callback) {
        await this.#child("context", name, callback, (child) =>
            new SelectTestContext(this.selectPath.slice(1), child.children)
        );
    }

    async #child(expectedType, expectedName, callback, createChildCtx) {
        if (this.childIdx === this.selectPath[0]) {
            const child = this.children[this.childIdx];
            assertEq(child.type, expectedType);
            assertEq(child.name, expectedName);

            const childCtx = createChildCtx(child);
            try {
                await callback(childCtx);
            } finally {
                childCtx.close();
            }
        }
        this.childIdx += 1;
    }
}

class ToplevelContext extends TestContext {
    async context(contextName, callback) {
        const probeCtx = new ProbeTestContext();
        try {
            await callback(probeCtx);
        } catch (e) {
            reportFail(`${contextName} (context)`, e);
            return;
        }

        function* dfs(parentPath, parentName, children) {
            for (let i = 0; i < children.length; ++i) {
                const path = parentPath.concat([i]);
                const child = children[i];
                const name = `${parentName} / ${child.name}`;
                if (child.type === "case") {
                    yield {path, name};
                } else if (child.type === "context") {
                    yield* dfs(path, name, child.children);
                }
            }
        }

        for (const {path, name} of dfs([], contextName, probeCtx.children)) {
            const selectCtx = new SelectTestContext(path, probeCtx.children);
            try {
                await callback(selectCtx);
            } catch (e) {
                reportFail(name, e);
                continue;
            } finally {
                selectCtx.close();
            }
            reportPass(name);
        }
    }
}

globalThis.failCount = 0;
globalThis.passCount = 0;

function reportFail(what, e) {
    println(`TEST ${what}: FAIL ${e.stack}`);
    globalThis.failCount += 1;
}

function reportPass(what) {
    println(`TEST ${what}: PASS`);
    globalThis.passCount += 1;
}

const t = new ToplevelContext();
