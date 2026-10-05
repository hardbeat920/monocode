# Linux locale and provider repair

The scoped Linux run passed after the shared ICU module and provider comparators were added. It rebuilt the common workspace library and binary graph and checked that all four provider catalog regressions were registered in the emitted harness test executable.

The [provider log](provider-catalog.log) records four passed regressions. The [full harness log](harness.log) records 777 passed and 35 ignored. The [locale log](locale.log) records 14 passed native locale tests. The [Clippy log](clippy.log) records a passing workspace check with all targets, all features and `-D warnings`.

The [default locale report](default-locale.json) compares the compiled native example with owned Node v24.21.0 and ICU 78.3. All eight cases matched both the collation results and relative phrase. They include the inherited host environment, `C`, `C.UTF-8`, `POSIX`, English, French and both conflicting `LANGUAGE` cases. Only child environments changed. The French case returned `il y a 2 heures` and the other cases returned `2 hours ago`.

The [summary](summary.json) records exact commands, zero exit codes and the emitted harness test binary hash. The [source record](source-sha256.json) records the 45 staged files. The runner verified those files before and after the tests and touched them before compilation. The three later locale import-format changes were not applied during this run. They do not change behavior and will enter the next complete source qualification.

This run qualifies the shared locale module and provider catalogs. It uses the prior frozen caller source with updated manifests. It does not qualify the new engine and UI caller repairs or a rebuilt application package.
