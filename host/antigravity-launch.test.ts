import { expect, it, vi } from "vitest";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { antigravityArgs, providerLaunch } from "./process";
import { HostChildBackend } from "./child-backend";

it.each(["cmd", "bat"])(
  "resolves simple agy.%s forwarders without evaluating batch text",
  async (extension) => {
    const directory = mkdtempSync(join(tmpdir(), "monocode-agy-forward-"));
    const target = join(directory, "runtime", "agy.exe");
    const wrapper = join(directory, `AGY.${extension.toUpperCase()}`);
    mkdirSync(join(directory, "runtime"));
    writeFileSync(target, "native fixture");
    const args = [
      "acp",
      "path with spaces",
      "a&b",
      "a|b",
      "%USERPROFILE%",
      'a"b',
      "line\nbreak",
    ];
    try {
      for (const forwardingTarget of [
        target,
        `%~dp0${join("runtime", "agy.exe")}`,
      ]) {
        writeFileSync(wrapper, `@echo off\r\n@"${forwardingTarget}" %*\r\n`);
        expect(await providerLaunch(wrapper, args, "win32")).toEqual({
          command: target,
          args,
        });
      }
      expect(antigravityArgs(wrapper, "win32")).toEqual(["acp"]);
      expect(antigravityArgs(join(directory, "AGY.COM"), "win32")).toEqual([
        "acp",
      ]);
      expect(
        await providerLaunch(join(directory, "AGY.COM"), args, "win32"),
      ).toEqual({ command: join(directory, "AGY.COM"), args });
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
);

it("rejects shell logic, argument rewrites, unknown expansions, and non-Agy targets", async () => {
  const directory = mkdtempSync(join(tmpdir(), "monocode-agy-reject-"));
  const wrapper = join(directory, "agy.cmd");
  const target = join(directory, "agy.exe");
  const sentinel = join(directory, "executed");
  writeFileSync(target, "native fixture");
  try {
    for (const source of [
      `"${target}" %* & echo bad > "${sentinel}"`,
      `echo bad > "${sentinel}"\n"${target}" %*`,
      `call "${target}" %*`,
      `"${target}" --dangerously-skip-permissions %*`,
      '"%LOCALAPPDATA%/agy/bin/agy.exe" %*',
      '"%~dp0node.exe" %*',
      '"agy.exe" %*',
      "x".repeat(16 * 1024 + 1),
    ]) {
      writeFileSync(wrapper, source);
      await expect(providerLaunch(wrapper, ["acp"], "win32")).rejects.toThrow(
        "Unsupported",
      );
      expect(existsSync(sentinel)).toBe(false);
    }
    await expect(
      providerLaunch(join(directory, "unknown.bat"), [], "win32"),
    ).rejects.toThrow("Unsupported Windows provider launcher");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

it.skipIf(process.platform !== "win32").each(["cmd", "bat"])(
  "discovers and starts a host with only an agy.%s launcher using literal argv and the workspace",
  async (extension) => {
    const directory = mkdtempSync(join(tmpdir(), "monocode agy & wrapper-"));
    const cwd = join(directory, "workspace");
    const target = join(directory, "runtime", "agy.exe");
    const wrapper = join(directory, `agy.${extension}`);
    mkdirSync(join(directory, "runtime"));
    mkdirSync(cwd);
    // Node renamed as agy.exe runs this workspace-local ACP fixture, without a model request.
    copyFileSync(process.execPath, target);
    writeFileSync(
      join(cwd, "acp"),
      "console.log(JSON.stringify({args:process.argv.slice(2),cwd:process.cwd()}));",
    );
    writeFileSync(
      wrapper,
      `@echo off\r\n@"%~dp0${join("runtime", "agy.exe")}" %*\r\n`,
    );
    for (const variable of ["PATH", "LOCALAPPDATA", "APPDATA", "USERPROFILE"])
      vi.stubEnv(variable, directory);
    const backend = new HostChildBackend();
    let output: unknown;
    const unlisten = await backend.listen<{ line: string }>(
      "harness-stdout",
      ({ payload }) => {
        output = JSON.parse(payload.line);
      },
    );
    try {
      const resolved = await backend.invoke<{ path: string; args: string[] }>(
        "harness_resolve_antigravity",
      );
      expect(resolved).toEqual({ path: wrapper, args: ["acp"] });
      const args = ["path with spaces", "a&b", "a|b", "%USERPROFILE%", 'a"b'];
      await backend.invoke("harness_spawn", {
        sessionId: "agy-wrapper",
        command: resolved.path,
        args: [...resolved.args, ...args],
        cwd,
        binaryProvider: "antigravity",
      });
      await vi.waitFor(() => expect(output).toEqual({ args, cwd }));
    } finally {
      unlisten();
      await backend.close();
      vi.unstubAllEnvs();
      rmSync(directory, { recursive: true, force: true });
    }
  },
);
