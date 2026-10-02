//! Port of `languageForPath` in src/features/files/editor/editorLanguage.ts.
//!
//! CodeMirror loads a Lezer grammar or a legacy stream mode per extension.
//! Here every language maps to a tree-sitter grammar that gpui-component
//! registers, by its registry name.

/// The gpui-component language name for a file, or `None` for plain text.
pub fn language_for_path(path: &str) -> Option<&'static str> {
    let name = basename(path).to_lowercase();
    let extension = name.rfind('.').map_or("", |index| &name[index..]);

    if matches!(extension, ".js" | ".jsx" | ".mjs" | ".cjs") {
        // tree-sitter-javascript parses JSX in plain `.js` files too.
        return Some("javascript");
    }
    if extension == ".ts" {
        return Some("typescript");
    }
    if extension == ".tsx" {
        return Some("tsx");
    }
    if extension == ".json" || name == "package-lock.json" {
        return Some("json");
    }
    if extension == ".jsonc" {
        // tree-sitter-json accepts comments, so JSONC needs no separate mode.
        return Some("json");
    }
    if extension == ".css" {
        return Some("css");
    }
    if matches!(extension, ".html" | ".htm") {
        return Some("html");
    }
    if matches!(extension, ".md" | ".mdx" | ".markdown") {
        return Some("markdown");
    }
    if extension == ".rs" {
        return Some("rust");
    }
    if extension == ".py" {
        return Some("python");
    }
    if extension == ".c" {
        return Some("c");
    }
    if matches!(
        extension,
        ".h" | ".cc" | ".cpp" | ".cxx" | ".hh" | ".hpp" | ".hxx"
    ) {
        return Some("cpp");
    }
    if extension == ".java" {
        return Some("java");
    }
    if matches!(extension, ".php" | ".phtml") {
        return Some("php");
    }
    if extension == ".sql" {
        return Some("sql");
    }
    if matches!(extension, ".xml" | ".svg") {
        // TODO(port): no tree-sitter XML grammar in gpui-component; the HTML
        // grammar tokenizes tags, attributes, and strings the same way.
        return Some("html");
    }
    if matches!(extension, ".yaml" | ".yml") {
        return Some("yaml");
    }
    if extension == ".cs" {
        return Some("csharp");
    }
    if extension == ".go" {
        return Some("go");
    }
    if extension == ".dart" {
        // TODO(port): no Dart grammar. CodeMirror used its C-like mode; the
        // Java grammar is the closest C-like grammar available.
        return Some("java");
    }
    if extension == ".swift" {
        return Some("swift");
    }
    if matches!(extension, ".kt" | ".kts") {
        return Some("kotlin");
    }
    if matches!(extension, ".rb" | ".rake") || matches!(name.as_str(), "gemfile" | "rakefile") {
        return Some("ruby");
    }
    if matches!(extension, ".sh" | ".bash" | ".zsh")
        || matches!(
            name.as_str(),
            ".bashrc" | ".bash_profile" | ".zshrc" | ".zprofile"
        )
    {
        return Some("bash");
    }
    if extension == ".toml" {
        return Some("toml");
    }
    if matches!(extension, ".scala" | ".sc") {
        return Some("scala");
    }
    if extension == ".lua" {
        return Some("lua");
    }
    if matches!(extension, ".r" | ".pl" | ".pm" | ".ps1" | ".psd1" | ".psm1") {
        // TODO(port): no R, Perl, or PowerShell grammar. They share `#`
        // comments and quoted strings with shell, so the Bash grammar stands in.
        return Some("bash");
    }
    if extension == ".m" {
        // TODO(port): no Objective-C grammar; C covers its C subset.
        return Some("c");
    }
    if extension == ".mm" {
        return Some("cpp");
    }
    if extension == ".proto" {
        return Some("proto");
    }
    if name == "dockerfile" || name.starts_with("dockerfile.") {
        // TODO(port): no Dockerfile grammar; shell covers comments and RUN lines.
        return Some("bash");
    }
    None
}

/// The last path component, for `/` and `\` separators.
pub fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_highlighting_for_reported_language_files() {
        for path in [
            "index.php",
            "Program.cs",
            "main.go",
            "app.dart",
            "View.swift",
            "Main.kt",
            "library.c",
            "library.hpp",
            "Application.java",
        ] {
            assert!(language_for_path(path).is_some(), "{path}");
        }
    }

    #[test]
    fn maps_c_files_to_c_while_keeping_headers_mapped_to_cpp() {
        assert_eq!(language_for_path("library.c"), Some("c"));
        assert_eq!(language_for_path("library.h"), Some("cpp"));
    }

    #[test]
    fn loads_highlighting_for_additional_mainstream_files() {
        for path in [
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
        ] {
            assert!(language_for_path(path).is_some(), "{path}");
        }
    }

    #[test]
    fn leaves_unknown_file_types_as_plain_text() {
        assert_eq!(language_for_path("notes.unknown"), None);
    }

    #[test]
    fn matches_names_case_insensitively_and_by_basename() {
        assert_eq!(language_for_path("/repo/src/App.TSX"), Some("tsx"));
        assert_eq!(language_for_path("C:\\repo\\Gemfile"), Some("ruby"));
        assert_eq!(language_for_path("/home/me/.zshrc"), Some("bash"));
    }
}
