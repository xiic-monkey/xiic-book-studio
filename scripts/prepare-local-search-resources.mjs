import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");
const resourcesRoot = path.join(repoRoot, "src-tauri", "resources");

const model = {
  id: "BAAI/bge-small-zh-v1.5",
  version: "bge-small-zh-v1.5",
  files: {
    "config.json": "3853a7979202c348751b753e36f579c41d8da7d36af617d3d907e1fc9b441f2a",
    "tokenizer.json": "48cea5d44424912a6fd1ea647bf4fe50b55ab8b1e5879c3275f80e339e8fae26",
    "special_tokens_map.json": "b6d346be366a7d1d48332dbc9fdf3bf8960b5d879522b7799ddba59e76237ee3",
    "pytorch_model.bin": "7c5fe667bbed05dc10e246e229b701ad266fe4d95ab946e9e5aa402056611b88",
  },
};

const sqliteVec = {
  version: "0.1.9",
  assets: {
    "aarch64-apple-darwin": {
      name: "sqlite-vec-0.1.9-loadable-macos-aarch64.tar.gz",
      sha256: "8282126333399ddfe98bbbcc7a1936e7252625aac49df056a98be602e46bfd29",
      binarySha256: "193e480c50b59a55977d166f4aaf0e1bc8832d6963516e5950f39e4d2ce0b793",
    },
    "x86_64-apple-darwin": {
      name: "sqlite-vec-0.1.9-loadable-macos-x86_64.tar.gz",
      sha256: "53ad76e400786515e2edcaed2f01271dda846316390b761fadbd2dcf56aa4713",
    },
  },
};

function sha256(buffer) {
  return createHash("sha256").update(buffer).digest("hex");
}

function log(message) {
  console.log(`[local-search] ${message}`);
}

function targetTriple() {
  if (process.env.XIIC_BUNDLE_TARGET_TRIPLE) {
    return process.env.XIIC_BUNDLE_TARGET_TRIPLE;
  }
  if (process.platform !== "darwin") {
    throw new Error("Local search packaging currently supports macOS only.");
  }
  if (process.arch === "arm64") {
    return "aarch64-apple-darwin";
  }
  if (process.arch === "x64") {
    return "x86_64-apple-darwin";
  }
  throw new Error(`Unsupported macOS architecture: ${process.arch}`);
}

async function download(url, destination, expectedSha256) {
  let lastError;
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    try {
      const response = await fetch(url);
      if (!response.ok) {
        throw new Error(`Download failed (${response.status}): ${url}`);
      }
      const payload = Buffer.from(await response.arrayBuffer());
      const actualSha256 = sha256(payload);
      if (actualSha256 !== expectedSha256) {
        throw new Error(
          `Checksum mismatch for ${path.basename(destination)}: expected ${expectedSha256}, got ${actualSha256}`,
        );
      }
      mkdirSync(path.dirname(destination), { recursive: true });
      const partial = `${destination}.partial`;
      writeFileSync(partial, payload);
      renameSync(partial, destination);
      return;
    } catch (error) {
      lastError = error;
      if (attempt < 3) {
        await new Promise((resolve) => setTimeout(resolve, attempt * 1000));
      }
    }
  }
  throw lastError;
}

async function ensureModel() {
  const destination = path.join(resourcesRoot, "models", model.version);
  mkdirSync(destination, { recursive: true });
  for (const [file, expectedSha256] of Object.entries(model.files)) {
    const output = path.join(destination, file);
    if (existsSync(output) && sha256(readFileSync(output)) === expectedSha256) {
      log(`Using cached ${model.version}/${file}`);
      continue;
    }
    log(`Downloading ${model.version}/${file}`);
    await download(
      `https://huggingface.co/${model.id}/resolve/main/${file}?download=true`,
      output,
      expectedSha256,
    );
  }
  writeFileSync(
    path.join(destination, "manifest.json"),
    `${JSON.stringify({ checksums: model.files }, null, 2)}\n`,
  );
}

function findFile(root, filename) {
  const result = spawnSync("find", [root, "-name", filename, "-type", "f", "-print", "-quit"], {
    encoding: "utf8",
  });
  if (result.status !== 0 || !result.stdout.trim()) {
    throw new Error(`Could not locate ${filename} in downloaded sqlite-vec archive.`);
  }
  return result.stdout.trim();
}

async function ensureSqliteVec() {
  const triple = targetTriple();
  const asset = sqliteVec.assets[triple];
  if (!asset) {
    throw new Error(`sqlite-vec has no configured bundle for ${triple}.`);
  }
  const output = path.join(resourcesRoot, "sqlite-vec", "vec0.dylib");
  if (
    existsSync(output) &&
    (!asset.binarySha256 || sha256(readFileSync(output)) === asset.binarySha256)
  ) {
    log(`Using cached sqlite-vec ${sqliteVec.version} for ${triple}`);
    return;
  }

  const temporaryRoot = mkdtempSync(path.join(tmpdir(), "xiic-book-studio-vec-"));
  try {
    const archive = path.join(temporaryRoot, asset.name);
    log(`Downloading sqlite-vec ${sqliteVec.version} for ${triple}`);
    await download(
      `https://github.com/asg017/sqlite-vec/releases/download/v${sqliteVec.version}/${asset.name}`,
      archive,
      asset.sha256,
    );
    const extractRoot = path.join(temporaryRoot, "extract");
    mkdirSync(extractRoot, { recursive: true });
    const extracted = spawnSync("tar", ["-xzf", archive, "-C", extractRoot], { encoding: "utf8" });
    if (extracted.status !== 0) {
      throw new Error(`Could not extract sqlite-vec: ${extracted.stderr || extracted.stdout}`);
    }
    const extension = findFile(extractRoot, "vec0.dylib");
    mkdirSync(path.dirname(output), { recursive: true });
    copyFileSync(extension, output);
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

await ensureModel();
await ensureSqliteVec();
