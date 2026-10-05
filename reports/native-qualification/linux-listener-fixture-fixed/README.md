# Linux listener fixture correction

The Linux suite compiled frozen source digest `58c060e4687dc6bf600d89ce6423f2682e25355ea2867406ef3d996bb544a4c0` and registered every required regression. The parallel run failed only `host::listener::tests::closing_releases_the_port_for_a_new_listener`. Its assertion tried to connect to an ephemeral port after the fixture released it. Another parallel listener can reuse that port. The same compiled fixture passed 30 isolated repetitions. The original failure and repetitions remain in [the baseline logs](../linux-frozen-58c060e4/workspace.log) and [the repetition log](../linux-frozen-58c060e4/listener-standalone-repeat.log).

The correction removes that connect-refused assertion. It retains the bounded same-port rebind and successful HTTP request, and explicitly checks the rebound port. Production listener behavior is unchanged. The corrected file SHA256 is `330e1947b04d767e5fbc9abf39b7657cbc92fc032aaf0b09f2c48fab239c367e`.

The focused case passed once in the full workspace feature graph. The freshly compiled remote binary passed 103 tests with four ignored tests. Whole-workspace, all-target, all-feature Clippy passed with warnings denied. Formatting also passed. See [focused.log](focused.log), [remote.log](remote.log), [clippy.log](clippy.log), [source-sha256.txt](source-sha256.txt), and [test-binary-sha256.txt](test-binary-sha256.txt).

The source delta check records this fixture as the only changed frozen input. This run does not claim qualification of the later localization repairs or regenerated packages.
