# Debugging

The Deno runtime that ChiselStrike server uses under the hood supports the V8 Inspector Protocol, which allows you to attach a debugger to a running application.

To activate the inspector, pass the `--inspect` flag as follows:

```
npm run dev -- --inspect
```

You will see the ChiselStrike server output something like:

```
Debugger listening on ws://127.0.0.1:9229/ws/c02bb41e-4286-4be0-b1e0-0f303afa9153
Visit chrome://inspect to connect to the debugger.
```

To attach a debugger, install Google Chrome, and go to the <a href="chrome://inspect">chrome://inspect</a> URL, and pick the ChiselStrike server to attach to.

:::info

The inspector integration is currently useful for CPU and heap profiling.
You cannot configure a ChiselStrike project as a workspace so you cannot set breakpoints to application code.
This is a <a href="https://github.com/chiselstrike/chiselstrike/issues/1620">limitation</a> in ChiselStrike server and will be lifted in a future release.
:::
