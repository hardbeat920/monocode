import { spawn } from "node:child_process";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath, pathToFileURL } from "node:url";

const workspace = fileURLToPath(new URL("../", import.meta.url));

function track(child) {
  // Attach immediately: short-lived processes can exit before a caller awaits them.
  child.done = new Promise((resolve) => {
    child.once("error", (error) => resolve({ code: 1, error }));
    child.once("close", (code, signal) => resolve({ code, signal }));
  });
  return child;
}

export async function startDev({
  root = workspace,
  args = [],
  platform = process.platform,
  spawnProcess = spawn,
  env = process.env,
  log = (message) => console.log(`[dev] ${message}`),
  debounce = 300,
} = {}) {
  const bundle = platform === "darwin";
  const buildEnv = { ...env, CARGO_BUILD_JOBS: env.CARGO_BUILD_JOBS || "1" };
  const launch = (command, argv, options = {}) => track(spawnProcess(command, argv, {
    cwd: root, env: buildEnv, stdio: "inherit", ...options,
  }));
  let app;
  let command;
  let closing = false;
  let closePromise;
  let didClose;
  const closed = new Promise((resolve) => { didClose = resolve; });
  let building;
  let timer;
  let revision = 0;

  async function stopApp() {
    const previous = app;
    if (!previous) return;
    app = undefined;
    // The opt-in debug hook invokes the app's normal persistence/provider cleanup.
    previous.stdin.end("quit\n");
    const force = setTimeout(() => {
      log("App did not finish shutting down within 10s; terminating it.");
      previous.kill("SIGKILL");
    }, 10_000);
    try { await previous.done; } finally { clearTimeout(force); }
  }

  async function run(commandName, argv, capture = false) {
    command = launch(commandName, argv, capture ? { stdio: ["ignore", "pipe", "inherit"] } : {});
    let stdout = "";
    command.stdout?.on("data", (data) => { stdout += data; });
    const result = await command.done;
    command = undefined;
    if (result.error) log(result.error.message);
    return { ...result, stdout };
  }

  async function rebuild() {
    if (closing || building) return;
    building = (async () => {
      let builtRevision;
      do {
        clearTimeout(timer);
        builtRevision = revision;
        const started = Date.now();
        log("Building… (the current app stays open)");
        const metadataResult = await run("cargo", ["metadata", "--manifest-path", "crates/monocode-gpui/Cargo.toml", "--locked", "--no-deps", "--format-version", "1"], true);
        if (closing) return;
        if (metadataResult.code !== 0) {
          log("Cargo metadata failed. Fix the error, then type r to retry.");
          continue;
        }
        const metadata = JSON.parse(metadataResult.stdout);
        const pkg = metadata.packages.find((pkg) => pkg.name === "monocode-gpui");
        if (!pkg) throw new Error("The monocode-gpui package was not found.");
        const target = buildEnv.CARGO_BUILD_TARGET
          ? join(metadata.target_directory, buildEnv.CARGO_BUILD_TARGET)
          : metadata.target_directory;
        const result = await run("cargo", ["build", "--manifest-path", "crates/monocode-gpui/Cargo.toml", "--locked", "-p", pkg.name, "--bin", pkg.name, "--profile", "native-dev"]);
        if (closing) return;
        if (result.code !== 0) {
          log("Build failed. Keeping the current app; type r to retry after fixing the error.");
          continue;
        }
        // Another restart request during compilation gets a fresh build first.
        if (builtRevision !== revision) continue;
        let executable = join(target, "native-dev", platform === "win32" ? "monocode-gpui.exe" : "monocode-gpui");
        if (bundle) {
          const dataMode = args.reduce((mode, arg, index) => {
            if (["--cwd", "--monocode-db"].includes(args[index - 1])) return mode;
            if (arg === "--dev-data") return "dev-data";
            if (arg === "--monocode-data") return "monocode-data";
            return mode;
          }, "dev-data");
          const packaged = await run("bash", ["scripts/bundle-native-macos.sh", "native-dev", target, pkg.version, dataMode]);
          if (closing) return;
          if (packaged.code !== 0) {
            log("App packaging failed. Fix the error, then type r to retry.");
            continue;
          }
          executable = join(target, "native-dev", "Monocode Native.app", "Contents", "MacOS", "monocode-gpui");
        }
        if (builtRevision !== revision) continue;
        await stopApp();
        if (closing) return;
        if (builtRevision !== revision) continue;
        const next = launch(executable, args, {
          env: { ...buildEnv, MONOCODE_DEV_RELOAD: "1" },
          stdio: ["pipe", "inherit", "inherit"],
          // Ctrl+C reaches the runner, which lets the app run its quit hooks.
          detached: platform !== "win32",
        });
        // An app that exits while a restart is being requested may close its pipe first.
        next.stdin.on("error", () => {});
        app = next;
        next.done.then((result) => {
          if (app !== next) return;
          app = undefined;
          log(result.error ? result.error.message : `App exited (${result.signal ?? result.code}). Type r to rebuild and relaunch.`);
        });
        log(`Ready in ${((Date.now() - started) / 1000).toFixed(1)}s. Type r then Enter to rebuild and restart.`);
      } while (!closing && builtRevision !== revision);
    })();
    try { await building; } finally { building = undefined; }
  }

  function requestBuild() {
    if (closing) return;
    revision++;
    clearTimeout(timer);
    timer = setTimeout(() => {
      log("Rebuilding and restarting…");
      void rebuild().catch(fail);
    }, debounce);
  }

  function close() {
    if (closePromise) return closePromise;
    closing = true;
    clearTimeout(timer);
    closePromise = (async () => {
      // Cargo is a child of this runner; never interrupt another terminal's build.
      command?.kill("SIGTERM");
      await stopApp();
      await building?.catch(() => {});
      didClose();
    })();
    return closePromise;
  }

  function fail(error) {
    log(error.message);
    process.exitCode = 1;
    void close();
  }

  log("Type r then Enter to rebuild and restart; Ctrl+C to stop. File saves do not trigger builds.");
  void rebuild().catch(fail);
  return { requestBuild, close, closed };
}

export function parseDevArgs(argv) {
  const options = { args: [] };
  for (let index = 0; index < argv.length; index++) {
    const arg = argv[index];
    if (arg === "--") {
      options.args.push(...argv.slice(index + 1));
      break;
    }
    if (["--watch", "--bundle"].includes(arg)) {
      throw new Error(`${arg} is not supported. Use npm run native:dev, then type r and Enter to rebuild and restart.`);
    } else {
      options.args.push(arg);
      // Preserve values even when an app path happens to match a runner flag.
      if (["--cwd", "--monocode-db"].includes(arg) && index + 1 < argv.length) {
        options.args.push(argv[++index]);
      }
    }
  }
  return options;
}

async function main() {
  const dev = await startDev(parseDevArgs(process.argv.slice(2)));
  const input = createInterface({ input: process.stdin });
  void dev.closed.then(() => { input.close(); process.stdin.pause(); });
  input.on("line", (line) => { if (line.trim().toLowerCase() === "r") dev.requestBuild(); });
  let stopping = false;
  const stop = async () => {
    if (stopping) return;
    stopping = true;
    input.close();
    process.stdin.pause();
    await dev.close();
  };
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch((error) => { console.error(`[dev] ${error.message}`); process.exitCode = 1; });
}
