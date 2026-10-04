# Default locale selection evidence

Read-only probes on October 3, 2026 compared Node Intl with the task user's native Windows locale APIs and child-only Linux environment settings. The probes did not change the parent environment or host settings.

The [Windows report](windows-locale-system-selection.json) records `GetUserPreferredUILanguages` as `en-US` and `GetUserDefaultLocaleName` as `en-US`. Node v25.4.0 with ICU 77.1 resolved `en-US` for both the collator and relative formatter and returned `2 hours ago`. Microsoft's APIs return the [display language list](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-getuserpreferreduilanguages) and the [user default locale](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-getuserdefaultlocalename), respectively.

The [Linux report](linux-locale-system-selection.json) used owned Node v24.21.0 with ICU 78.3 on WSL Ubuntu. Both collator and relative formatter resolved `en-US` and returned `2 hours ago` in each case below.

| Child `LANGUAGE` | Child `LC_ALL` | Child `LANG` |
| --- | --- | --- |
| `fr_FR:de_DE` | `en_US.UTF-8` | `en_US.UTF-8` |
| `fr_FR` | `C` | `C` |

These cases expose a difference from a locale selector that prioritizes `LANGUAGE`. They do not qualify the repaired Rust default selector. The native example must still run against Node after the shared locale source is stable.

The [Windows keyword export log](windows-icu-keyword-capabilities.log) confirms the exact `uloc_getKeywordValue` and `uloc_setKeywordValue` symbols in the combined system ICU DLL. The legacy common DLL forwards both exports to the combined DLL on this Windows 11 target. This evidence does not establish availability on an older Windows installation.
