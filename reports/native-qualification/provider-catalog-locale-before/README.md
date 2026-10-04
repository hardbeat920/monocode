# Provider catalog locale before repair

The Windows test run failed all three added catalog regressions against the previous comparators. The source copy differed from the frozen `58c060e4` manifest only in the two protocol files, which added tests. It retained the old production comparators and Cargo graph.

The tests call the actual OpenCode model parser and catalog builder, OpenCode effort sorter, and PI and OMP RPC catalog builders. They show that the old comparators move canonically equivalent model names out of provider order and place accented names after `Zebra`.

The [raw native test log](tests-native-utf8.log) records zero passed and three failed. The [Cargo log](tests.log) records the common workspace library and binary graph command. The [source comparison](source-delta.log) records two changed files, zero missing files and zero extra files. The [source hashes](source-sha256.json) and [test binary hash](test-binary-sha256.txt) identify the tested inputs. The `before-source` directory preserves the two test-only source files.

The command was `cargo test --workspace --lib --bins --all-features --locked --offline -j2 catalog_locale -- --nocapture`. The retained [Node Intl oracle](retained-intl-reference-utf8.log) used the installed Windows Node v25.4.0 and resolved `en-US`. It returned the expected model and variant order. The oracle and native test output were captured as raw UTF-8 bytes. This avoids PowerShell 5 console conversion of accented text.

This is failing evidence before the locale repair. It does not qualify the repaired comparators, a fresh package or an interactive desktop.
