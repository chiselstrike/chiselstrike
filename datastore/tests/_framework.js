class ChildTestContext {
    constructor(name, iterIdx) {
        this.name = name;
        this.iterIdx = iterIdx;
        this.caseCount = 0;
    }

    async case(name, callback) {
        if (this.iterIdx === this.caseCount++) {
            const fullName = `${this.name} / ${name}`;
            await evalCase(fullName, callback);
        }
    }

    async context(name, callback) {
        if (this.iterIdx === 0) {
            const childName = `${this.name} / ${name}`;
            await evalContext(childName, callback);
        }
    }
}

class ToplevelContext {
    async case(name, callback) {
        await evalCase(name, callback);
    }

    async context(name, callback) {
        await evalContext(name, callback);
    }
}

async function evalCase(fullName, callback) {
    try {
        await callback();
    } catch (e) {
        reportFail(fullName, e);
        return;
    }
    reportPass(fullName);
}

async function evalContext(childName, callback) {
    const firstCtx = new ChildTestContext(childName, 0);
    try {
        await callback(firstCtx);
    } catch (e) {
        reportFail(childName, e);
        return;
    }

    for (let i = 1; i < firstCtx.caseCount; ++i) {
        const ithCtx = new ChildTestContext(childName, i);
        try {
            await callback(ithCtx);
        } catch (e) {
            reportFail(`${childName} (iter ${i})`, e);
            return;
        }
    }
}

failCount = 0;
passCount = 0;

function reportFail(what, e) {
    println(`TEST ${what}: FAIL ${e.stack}`);
    failCount += 1;
}

function reportPass(what) {
    println(`TEST ${what}: PASS`);
    passCount += 1;
}

const t = new ToplevelContext();
