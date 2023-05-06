#!/usr/bin/env node

const { run } = require("./binary");

if (process) {
  process.send("ready"); 
}

run();
