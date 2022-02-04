const jwt = require("jsonwebtoken");
const git = require("isomorphic-git");
const util = require("util");
const fetch = require("node-fetch-commonjs");
const http = require("isomorphic-git/http/node");
const fs = require("fs");
const path = require("path");

function die(msg) {
    throw msg;
}

async function cloneRepo() {
    const installation = process.env.INSTALLATION ??
        die("missing installation");
    const org = process.env.ORG ?? die("missing org");
    const repo = process.env.REPO ?? die("missing repo");
    const branchName = process.env.BRANCH ?? die("missing branch");
    const base = process.env.BASE_DIR ?? "./";

    const iat = Math.floor(Date.now() / 1000) - 30;
    const exp = iat + 600;
    const iss = process.env.APP_ID;

    const pem = fs.readFileSync(process.env.PRIVATE_KEY);
    const token = jwt.sign({ iat, exp, iss }, pem, {
        algorithm: "RS256",
    });

    const tokenUrl = util.format(
        "https://api.github.com/app/installations/%s/access_tokens",
        installation,
    );

    const tokenHeaders = {
        Accept: "application/vnd.github.v3+json",
        Authorization: util.format(" Bearer  %s", token),
    };

    const res = await fetch(tokenUrl, {
        method: "POST",
        headers: tokenHeaders,
    });
    const body = await res.json();
    if (res.status != 201) {
        throw body;
    }

    const userToken = body.token;
    const dir = path.join(base, repo);

    await git.clone({
        fs,
        http,
        dir,
        url: `https://github.com/${org}/${repo}`,
        onAuth: (url) => {
            return {
                username: "x-access-token",
                password: userToken,
            };
        },
        depth: 1,
        noTags: true,
        ref: branchName,
        singleBranch: true,
    });
    console.log(`cloned ${org}/${repo} into ${dir}`);
}

cloneRepo().catch((e) => console.error(`failed: ${e}`));
