import { language } from "@codemirror/language";
import { EditorState, type Extension } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { languageForPath } from "./editorLanguage";

function languageName(extension: Extension): string | null {
  return (
    EditorState.create({ extensions: [extension] }).facet(language)?.name ??
    null
  );
}

describe("languageForPath", () => {
  it.each([
    "index.php",
    "Program.cs",
    "main.go",
    "app.dart",
    "View.swift",
    "Main.kt",
    "library.c",
    "library.hpp",
    "Application.java",
  ])("loads highlighting for reported language file %s", async (path) => {
    await expect(languageForPath(path)).resolves.not.toBeNull();
  });

  it("maps C files to C while keeping headers mapped to C++", async () => {
    const c = await languageForPath("library.c");
    const header = await languageForPath("library.h");

    expect(c).not.toBeNull();
    expect(header).not.toBeNull();
    expect(languageName(c!)).toBe("c");
    expect(languageName(header!)).toBe("cpp");
  });

  it.each([
    "app.rb",
    "deploy.sh",
    "query.sql",
    "workflow.yaml",
    "layout.xml",
    "Cargo.toml",
    "build.scala",
    "plugin.lua",
    "analysis.r",
    "script.pl",
    "profile.ps1",
    "Controller.m",
    "messages.proto",
    "Dockerfile",
    "settings.jsonc",
  ])("loads highlighting for additional mainstream file %s", async (path) => {
    await expect(languageForPath(path)).resolves.not.toBeNull();
  });

  it("leaves unknown file types as plain text", async () => {
    await expect(languageForPath("notes.unknown")).resolves.toBeNull();
  });
});
