# Claude limit reset integration

Research date: 2026-10-02. The request shapes below were checked against the
embedded JavaScript in Anthropic's published Claude Code **2.1.288** Darwin ARM64
binary. The package was downloaded and inspected without executing it:
[official npm package](https://www.npmjs.com/package/@anthropic-ai/claude-code/v/2.1.288),
[platform package](https://registry.npmjs.org/@anthropic-ai/claude-code-darwin-arm64/2.1.288).
These are internal subscription endpoints, rather than the public Messages API.

## Discovery and redemption

The CLI has two reset programs:

| Program | Discovery read | Redemption body |
| --- | --- | --- |
| Saved grants (`cedar_ember`) | `GET /api/oauth/usage?cedar_ember=1&skip_spend=1` | `{ "program": "cedar_ember", "grant_id": "<selected grant>", "request_id": "<idempotency key>" }` |
| Session-only offer (`juniper_tide`) | `GET /api/oauth/usage?at_wall=1&skip_spend=1` | `{ "program": "juniper_tide" }` |

Both POST to `/api/organizations/<organization UUID>/reset_rate_limits` on
`https://api.anthropic.com`, using the account's OAuth bearer. MonoCode obtains
the organization from `/api/oauth/profile` with that same bearer; the CLI uses
its cached OAuth organization UUID. A Cedar grant declares its affected windows
in `clears`, its remaining count in `resets_left`, and its expiry in `ends_at`.
`next_grant_id` selects the grant the server permits spending; `usable_now`,
`eligible`, `paused`, start time, and cooldown constrain redemption. Juniper
requires the reset experiment arm and an available offer.

The plain usage response can include null program placeholders. Null does not
establish that the account has no resets: the explicit discovery read evaluates
the program. MonoCode reads Cedar during usage refresh and evaluates the at-wall
programs when the five-hour window is exhausted. Older servers rejecting the
Cedar query fall back to the plain usage endpoint and expose a discovery notice.

## Account and outcome handling

Redemption rechecks the exact confirmed offer and resolves the organization
under the selected account's current credential. A changed credential cancels
the request. Credential refresh and rotation remain owned by Claude Code.
Host requests do not follow redirects, limit response size, and serialize reset
operations across windows. A POST is never automatically retried. Cedar keeps
its idempotency key across an uncertain result, until a conclusive outcome.

Only `result: "reset"` reports an applied reset. `not_limited` and `already_used`
have separate messages. Cooldown, ineligibility, malformed responses, HTTP
failures, and transport uncertainty do not report success. Usage and reset
inventory are reloaded after the operation; MonoCode does not guess which
model quotas were replenished or change their scheduled reset times.

## Web-only offers

Anthropic's [help article](https://support.claude.com/en/articles/17007452-what-is-a-limit-reset)
documents redemption through Claude Web/Desktop and says redemption there also
restores the shared Claude Code allowance. The CLI binary additionally contains
the OAuth paths above. An account can still return `ineligible_reason: "surface"`
for an offer shown on the web. MonoCode respects that response, explains the
restriction, and links to Claude's usage page rather than attempting to spend
an offer through a different authentication surface.

Verification uses synthetic HTTP servers, parser fixtures, and UI tests. No live
account reset was consumed during development.
