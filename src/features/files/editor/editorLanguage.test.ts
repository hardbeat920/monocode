import { describe, expect, it } from "vitest";
import { languageForPath } from "./editorLanguage";

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
  ])("loads highlighting for additional mainstream file %s", async (path) => {
    await expect(languageForPath(path)).resolves.not.toBeNull();
  });

  it("leaves unknown file types as plain text", async () => {
    await expect(languageForPath("notes.unknown")).resolves.toBeNull();
  });
});
