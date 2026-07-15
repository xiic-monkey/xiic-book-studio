import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const children = [];
let exiting = false;

function start(name, command, args, extraEnv = {}, cwd = process.cwd()) {
  const child = spawn(command, args, {
    stdio: "inherit",
    env: {
      ...process.env,
      ...extraEnv,
    },
    cwd,
    shell: false,
  });

  child.on("exit", (code, signal) => {
    if (exiting) return;
    exiting = true;
    for (const process of children) {
      if (process.pid && process.pid !== child.pid) {
        process.kill("SIGTERM");
      }
    }

    if (signal) {
      console.error(`${name} exited via ${signal}`);
      process.exit(1);
    }
    process.exit(code ?? 0);
  });

  children.push(child);
}

function shutdown() {
  if (exiting) return;
  exiting = true;
  for (const child of children) {
    if (child.pid) {
      child.kill("SIGTERM");
    }
  }
  setTimeout(() => process.exit(0), 50);
}

process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);

start("dev-api", "cargo", ["run", "--bin", "dev_api"], {
  CARGO_TERM_COLOR: "always",
}, fileURLToPath(new URL("../src-tauri/", import.meta.url)));

start("vite", "npm", ["run", "dev:web"], {
  VITE_DEV_API_BASE: process.env.VITE_DEV_API_BASE ?? "http://127.0.0.1:4141",
});
