# M0 literal JSON byte check

The existing core and layout golden tests compare JSON values and normalize numbers. They establish field and content preservation, but they do not satisfy the plan's literal byte-equivalent JSON requirement. Keep that requirement unchanged.

## Source and copied-data findings

The original online backup recorded in `data-compat/backup.json` has SHA-256 `37aa3b11321068debeb380b75c793792cb816fab9f9baa76ef2fb994dbf192a7`. The private `golden-cli.db` copy still matches that hash and has DELETE journal mode. It opens read-only without WAL sidecars. Other copies used by later UI checks now have different database hashes and WAL headers. Use the untouched matching copy for this literal check. No database changed during this inspection, and no user content was uploaded.

Read-only inspection of the matching copy found 126 sessions and 31,985 blocks. Every populated JSON column is compact and key-sorted already:

| Column | Populated rows | Original bytes equal a compact sorted JSON encoding |
| --- | ---: | ---: |
| `blocks_json` | 126 | 126 |
| `model_settings` | 126 | 126 |
| `linked_work_item_json` | 3 | 3 |
| `inbox_ask` | 0 | No real rows to cover |

Those counts came from Python's JSON parser and are format evidence only. They are not a passing Rust typed-byte round trip. SQLite `quick_check` returned `ok` for the matching copy.

`Block` derives serialization in declaration order, starting with `id`, `role`, and `text`. Stored blocks have sorted key order, often starting with `durationMs`. Direct `serde_json::to_string(Vec<Block>)` therefore changes key order even when it preserves content.

Native persistence uses a different concrete encoding. `runtime::session_store::sanitize_session_for_persist` converts typed blocks and model settings to `serde_json::Value`. `store::session_store::upsert_session` serializes those values into the JSON text columns. The workspace uses the default sorted `serde_json::Map`, without `preserve_order`.

The stored copy contains 533 `cacheHitPercent` values. All are fractional. The typed field is `Option<f64>`, so a strict fixture must also cover an integer-valued percentage and reject an output change such as `12` becoming `12.0`. The current numeric normalization would accept that change.

## Exact assertion

[The fixture](../../crates/core/tests/golden_json_bytes.rs) deserializes each populated JSON column into its core type, serializes it through the existing persistence value encoder, and compares the resulting UTF-8 bytes directly to the untouched original string. It does not parse, sort, normalize, or replace the original string before comparison. It prints only counts, lengths, and the first differing byte offset. It does not log session IDs or content.

Its two deterministic tests passed. They require whitespace, key order, and numeric spelling changes to fail the comparison. This makes the check stricter than value equality and ties its output format to the actual native persistence encoding. Root promoted the proposal into the core test suite and compiled the actual test on the remote build machine.

The private-copy check passed locally. All 126 transcript columns, 126 model-settings columns, and three linked-item columns matched byte for byte. The copy contained no populated Inbox-context columns. [The strict log](data-compat/json-bytes.log) records zero failures. The read-only check left the backup hash unchanged. This satisfies the literal M0 session JSON requirement for that copied database.

The test executable had SHA-256 `bb7c7c2c4da3ada23516e32e9ddc3c5cfdcc54776d93821750d0be7c029ded49`. The database stayed on the local Mac. Only the source and test executable traveled between build machines.

Run the two deterministic tests with the normal core integration tests. Run the ignored private-copy assertion explicitly:

```sh
MONOCODE_GOLDEN_DB=/tmp/mc/native-qa/data-compat/golden-cli.db cargo test -p monocode-core --test golden_json_bytes --locked every_session_json_column_is_byte_equivalent -- --ignored --nocapture
```

The M0 gate passes only if all populated columns compare byte for byte. The private-copy test is ignored during normal tests. Running it explicitly without `MONOCODE_GOLDEN_DB` fails. An ignored fixture is not gate evidence. If the strict run fails, preserve the original strings and report the mismatch. Do not overwrite the copied JSON with a canonical encoding to obtain a pass.

## Persistence qualification beyond that gate

A typed-byte round trip does not prove that opening and saving through the full engine leaves every stored column unchanged. A separate no-op persistence regression should open a disposable clone, capture all original JSON bytes, hydrate and persist each session through the real engine and store APIs, then compare the stored JSON columns directly. This may reveal deliberate sanitization or a no-op update that changes numeric spelling. It must leave the original backup intact.

The store currently regenerates `blocks_json`, `model_settings`, and linked-item JSON on every upsert. Its `json_eq` check preserves `updated_at` when block values are equal, but it does not reuse the original text. If a no-op upsert changes bytes, fix the persistence behavior with the mismatch as a regression. A semantic comparison alone does not close the literal gate.
