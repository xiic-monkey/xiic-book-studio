import { createHash } from "node:crypto";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");
const resourcesRoot = path.join(repoRoot, "src-tauri", "resources");
const destination = path.join(resourcesRoot, "web-search-mcp");
const revision = "eeb03f88525cbf74c4019e59a3fea45a537a760b";
const lockfileSha256 = "077c23c3d3d9e0a99fb4ef1820017a5d82ae3ae624c68001d0f12f9e711700ca";
const source = "https://github.com/mrkrsl/web-search-mcp.git";
const manifestName = "xiic-manifest.json";
const runtimeFormatVersion = 4;
const esmRequireBanner = 'import { createRequire as __xiicCreateRequire } from "node:module"; const require = __xiicCreateRequire(import.meta.url);';

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? repoRoot,
    env: { ...process.env, ...options.env },
    encoding: "utf8",
    input: options.input,
    timeout: options.timeout,
  });
  if (result.status !== 0 || result.error) {
    throw new Error(
      `${command} ${args.join(" ")} failed:\n${result.error?.message ?? result.stderr ?? result.stdout}`,
    );
  }
  return result.stdout.trim();
}

function ensureEsmRequireShim(entryPath) {
  const sourceText = readFileSync(entryPath, "utf8");
  if (sourceText.includes("__xiicCreateRequire")) return;
  const newline = sourceText.indexOf("\n");
  const patched = sourceText.startsWith("#!") && newline >= 0
    ? `${sourceText.slice(0, newline + 1)}${esmRequireBanner}\n${sourceText.slice(newline + 1)}`
    : `${esmRequireBanner}\n${sourceText}`;
  writeFileSync(entryPath, patched);
}

function smokeTestServer(root) {
  const node = path.join(root, "runtime", "node");
  const entry = path.join(root, "service", "index.mjs");
  const script = String.raw`
    import { spawn } from "node:child_process";
    const child = spawn(process.env.XIIC_SMOKE_NODE, [process.env.XIIC_SMOKE_ENTRY], {
      cwd: process.env.XIIC_SMOKE_ROOT,
      env: {
        ...process.env,
        PLAYWRIGHT_BROWSERS_PATH: process.env.XIIC_SMOKE_BROWSERS,
        BROWSER_HEADLESS: "true",
        BROWSER_TYPES: "chromium",
        MAX_BROWSERS: "1",
        DEFAULT_TIMEOUT: "3000",
      },
      stdio: ["pipe", "pipe", "pipe"],
    });
    let buffer = "";
    let stderr = "";
    const timer = setTimeout(() => finish(1, "MCP initialize timeout"), 10000);
    function finish(code, message = "") {
      clearTimeout(timer);
      child.kill();
      if (message) process.stderr.write(message + "\n" + stderr.slice(-1200));
      process.exit(code);
    }
    child.stderr.on("data", chunk => { stderr += chunk.toString(); });
    child.on("exit", code => { if (code !== null && code !== 0) finish(1, "MCP server exited " + code); });
    child.stdout.on("data", chunk => {
      buffer += chunk.toString();
      for (;;) {
        const newline = buffer.indexOf("\n");
        if (newline < 0) break;
        const line = buffer.slice(0, newline).trim();
        buffer = buffer.slice(newline + 1);
        let message;
        try { message = JSON.parse(line); } catch { continue; }
        if (message.id === 1) {
          if (message.error) finish(1, JSON.stringify(message.error));
          finish(0);
        }
      }
    });
    child.stdin.write(JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: {
        protocolVersion: "2024-11-05",
        capabilities: {},
        clientInfo: { name: "xiic-runtime-check", version: "1" },
      },
    }) + "\n");
  `;
  run(node, ["--input-type=module", "-e", script], {
    cwd: root,
    timeout: 15000,
    env: {
      XIIC_SMOKE_NODE: node,
      XIIC_SMOKE_ENTRY: entry,
      XIIC_SMOKE_ROOT: root,
      XIIC_SMOKE_BROWSERS: path.join(root, "browsers"),
    },
  });
}

function sha256(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

function copyDereferenced(sourcePath, destinationPath) {
  const sourceInfo = lstatSync(sourcePath);
  if (sourceInfo.isSymbolicLink()) {
    copyDereferenced(realpathSync(sourcePath), destinationPath);
    return;
  }
  if (sourceInfo.isDirectory()) {
    mkdirSync(destinationPath, { recursive: true });
    for (const entry of readdirSync(sourcePath)) {
      copyDereferenced(path.join(sourcePath, entry), path.join(destinationPath, entry));
    }
    chmodSync(destinationPath, sourceInfo.mode & 0o777);
    return;
  }
  copyFileSync(sourcePath, destinationPath);
  chmodSync(destinationPath, sourceInfo.mode & 0o777);
}

function makeOwnerWritable(root) {
  if (!existsSync(root)) return;
  const info = lstatSync(root);
  if (info.isSymbolicLink()) return;
  chmodSync(root, info.mode | 0o200 | (info.isDirectory() ? 0o100 : 0));
  if (!info.isDirectory()) return;
  for (const entry of readdirSync(root)) makeOwnerWritable(path.join(root, entry));
}

function normalizePermissions(root) {
  const info = lstatSync(root);
  if (info.isSymbolicLink()) {
    throw new Error(`Runtime resource still contains a symlink: ${root}`);
  }
  if (info.isDirectory()) {
    chmodSync(root, 0o755);
    for (const entry of readdirSync(root)) normalizePermissions(path.join(root, entry));
    return;
  }
  chmodSync(root, info.mode & 0o111 ? 0o755 : 0o644);
}

function assertRuntime(root, executeChecks) {
  const manifestPath = path.join(root, manifestName);
  const required = [
    manifestPath,
    path.join(root, "service", "index.mjs"),
    path.join(root, "runtime", "node"),
    path.join(root, "node_modules", "playwright", "package.json"),
    path.join(root, "node_modules", "playwright-core", "package.json"),
    path.join(root, "browsers"),
  ];
  for (const candidate of required) {
    if (!existsSync(candidate)) throw new Error(`Missing web-search runtime resource: ${candidate}`);
  }

  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  if (
    manifest.runtime_format_version !== runtimeFormatVersion ||
    manifest.revision !== revision ||
    manifest.lockfile_sha256 !== lockfileSha256
  ) {
    throw new Error("Web-search runtime manifest does not match the pinned source.");
  }

  for (const forbidden of ["src", "tests", "docs", "scripts", ".git", "package-lock.json", "README.md"]) {
    if (existsSync(path.join(root, forbidden))) {
      throw new Error(`Forbidden source/build asset leaked into runtime: ${forbidden}`);
    }
  }
  const runtimePackages = readdirSync(path.join(root, "node_modules"))
    .filter((name) => !name.startsWith("."))
    .sort();
  if (runtimePackages.join(",") !== "playwright,playwright-core") {
    throw new Error(`Unexpected runtime packages: ${runtimePackages.join(", ")}`);
  }

  normalizePermissions(root);
  chmodSync(path.join(root, "runtime", "node"), 0o755);
  const browserEntries = readdirSync(path.join(root, "browsers"));
  if (!browserEntries.some((name) => name.startsWith("chromium_headless_shell-"))) {
    throw new Error("Chromium headless-shell runtime is missing.");
  }
  if (browserEntries.some((name) => name.startsWith("chromium-") && !name.startsWith("chromium_headless_shell-"))) {
    throw new Error("The full headed Chromium build must not be bundled.");
  }

  if (executeChecks) {
    const node = path.join(root, "runtime", "node");
    run(node, ["--check", path.join(root, "service", "index.mjs")], { cwd: root });
    run(node, ["--input-type=module", "-e", "await import('playwright')"], { cwd: root });
    smokeTestServer(root);
  }
}

function isPrepared() {
  if (!existsSync(destination)) return false;
  try {
    makeOwnerWritable(destination);
    assertRuntime(destination, true);
    return true;
  } catch {
    return false;
  }
}

function upgradeVerifiedRuntimeCache() {
  const manifestPath = path.join(destination, manifestName);
  if (!existsSync(manifestPath)) return false;
  try {
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    if (manifest.revision !== revision || manifest.lockfile_sha256 !== lockfileSha256) return false;
    if (![3, runtimeFormatVersion].includes(manifest.runtime_format_version)) return false;
    ensureEsmRequireShim(path.join(destination, "service", "index.mjs"));
    for (const name of readdirSync(path.join(destination, "browsers"))) {
      if (name.startsWith("chromium-") && !name.startsWith("chromium_headless_shell-")) {
        const candidate = path.join(destination, "browsers", name);
        makeOwnerWritable(candidate);
        rmSync(candidate, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
      }
    }
    const links = path.join(destination, "browsers", ".links");
    makeOwnerWritable(links);
    rmSync(links, { recursive: true, force: true });
    writeFileSync(
      manifestPath,
      `${JSON.stringify({
        ...manifest,
        runtime_format_version: runtimeFormatVersion,
        entry: "service/index.mjs",
        node: "runtime/node",
        browsers: "browsers",
      }, null, 2)}\n`,
    );
    assertRuntime(destination, true);
    return true;
  } catch {
    return false;
  }
}

if (isPrepared() || upgradeVerifiedRuntimeCache()) {
  console.log(`[web-search-mcp] Using cached minimal runtime ${revision.slice(0, 12)}`);
  process.exit(0);
}

const stagingRoot = mkdtempSync(path.join(tmpdir(), "xiic-book-studio-web-search-"));
const checkout = path.join(stagingRoot, "checkout");
const runtimeBundle = path.join(stagingRoot, "runtime-bundle");
const destinationStaging = path.join(resourcesRoot, `.web-search-mcp-staging-${process.pid}`);
mkdirSync(checkout, { recursive: true });

try {
  run("git", ["init"], { cwd: checkout });
  run("git", ["remote", "add", "origin", source], { cwd: checkout });
  run("git", ["fetch", "--depth", "1", "origin", revision], { cwd: checkout });
  run("git", ["checkout", "--detach", "FETCH_HEAD"], { cwd: checkout });
  const checkedOut = run("git", ["rev-parse", "HEAD"], { cwd: checkout });
  if (checkedOut !== revision) throw new Error(`Unexpected web-search-mcp revision: ${checkedOut}`);
  const actualLockfileHash = sha256(path.join(checkout, "package-lock.json"));
  if (actualLockfileHash !== lockfileSha256) {
    throw new Error(`Pinned lockfile checksum mismatch: ${actualLockfileHash}`);
  }

  run("npm", ["ci", "--ignore-scripts"], { cwd: checkout });
  const searchEnginePath = path.join(checkout, "src", "search-engine.ts");
  const searchEngine = readFileSync(searchEnginePath, "utf8");
  const chromiumFallback = searchEngine.replace(
    "const { firefox } = await import('playwright');\n        browser = await firefox.launch({",
    "const { chromium } = await import('playwright');\n        browser = await chromium.launch({",
  );
  if (chromiumFallback === searchEngine) {
    throw new Error("Could not apply the Chromium fallback patch to web-search-mcp.");
  }
  writeFileSync(searchEnginePath, chromiumFallback);

  const browserPoolPath = path.join(checkout, "src", "browser-pool.ts");
  const browserPool = readFileSync(browserPoolPath, "utf8");
  const chromiumOnlyPool = browserPool.replace(
    "process.env.BROWSER_TYPES || 'chromium,firefox'",
    "process.env.BROWSER_TYPES || 'chromium'",
  );
  if (chromiumOnlyPool === browserPool) {
    throw new Error("Could not apply the Chromium-only browser pool patch.");
  }
  writeFileSync(browserPoolPath, chromiumOnlyPool);

  const bundledEntry = path.join(checkout, "dist", "xiic-web-search.mjs");
  run("npx", [
    "esbuild",
    "src/index.ts",
    "--bundle",
    "--platform=node",
    "--target=node20",
    "--format=esm",
    `--banner:js=${esmRequireBanner}`,
    `--outfile=${bundledEntry}`,
    "--external:playwright",
    "--external:playwright-core",
  ], { cwd: checkout });

  const browsers = path.join(checkout, "browsers");
  run(process.execPath, [path.join(checkout, "node_modules", "playwright", "cli.js"), "install", "chromium"], {
    cwd: checkout,
    env: { PLAYWRIGHT_BROWSERS_PATH: browsers },
  });

  mkdirSync(path.join(runtimeBundle, "service"), { recursive: true });
  mkdirSync(path.join(runtimeBundle, "runtime"), { recursive: true });
  mkdirSync(path.join(runtimeBundle, "node_modules"), { recursive: true });
  copyFileSync(bundledEntry, path.join(runtimeBundle, "service", "index.mjs"));
  copyFileSync(process.execPath, path.join(runtimeBundle, "runtime", "node"));
  copyFileSync(path.join(checkout, "LICENSE"), path.join(runtimeBundle, "LICENSE"));
  copyDereferenced(
    path.join(checkout, "node_modules", "playwright"),
    path.join(runtimeBundle, "node_modules", "playwright"),
  );
  copyDereferenced(
    path.join(checkout, "node_modules", "playwright-core"),
    path.join(runtimeBundle, "node_modules", "playwright-core"),
  );
  const runtimeBrowsers = path.join(runtimeBundle, "browsers");
  mkdirSync(runtimeBrowsers, { recursive: true });
  for (const name of readdirSync(browsers)) {
    if (!name.startsWith("chromium_headless_shell-") && !name.startsWith("ffmpeg-")) continue;
    copyDereferenced(path.join(browsers, name), path.join(runtimeBrowsers, name));
  }
  writeFileSync(
    path.join(runtimeBundle, manifestName),
    `${JSON.stringify({
      runtime_format_version: runtimeFormatVersion,
      revision,
      lockfile_sha256: lockfileSha256,
      entry: "service/index.mjs",
      node: "runtime/node",
      browsers: "browsers",
    }, null, 2)}\n`,
  );

  assertRuntime(runtimeBundle, true);
  rmSync(destinationStaging, { recursive: true, force: true });
  copyDereferenced(runtimeBundle, destinationStaging);
  assertRuntime(destinationStaging, true);

  makeOwnerWritable(destination);
  rmSync(destination, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
  renameSync(destinationStaging, destination);
  assertRuntime(destination, true);
  console.log(`[web-search-mcp] Prepared minimal runtime ${revision.slice(0, 12)}`);
  console.log(`[web-search-mcp] Size: ${Math.ceil(statSync(path.join(destination, "runtime", "node")).size / 1024 / 1024)} MiB Node runtime plus Chromium`);
} finally {
  makeOwnerWritable(destinationStaging);
  rmSync(destinationStaging, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
  makeOwnerWritable(stagingRoot);
  rmSync(stagingRoot, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
}
