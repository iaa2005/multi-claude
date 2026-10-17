# Claude Science in CC Switch

Status: **foundation** (this branch). `src-tauri/src/claude_science.rs` builds
and tests the environment/process half; the app type, proxy route and UI are
not wired yet.

## What Claude Science is

A scientific agent that ships as `claude-science.exe` (0.1.63 at the time of
writing): a Bun CLI that starts a local daemon (default `127.0.0.1:8000`) and
opens a browser UI. It signs in to claude.ai, stores state in
`~/.claude-science`, and fetches the model list over an Anthropic-style API
(`models-last-good.json` is keyed by `<base url>|org:<id>`).

It is **not** an Electron client with a provider config file. There is no
`claude_desktop_config.json` equivalent: the inference endpoint comes from the
daemon's environment.

## Does it support a third-party endpoint?

Yes, and deliberately so. Measured in the binary:

- reads `process.env.ANTHROPIC_BASE_URL`, `ANTHROPIC_API_KEY`,
  `ANTHROPIC_AUTH_TOKEN` and constructs the Anthropic client with that
  `baseURL`;
- validates the URL and rejects plain http **except literal loopback hosts**:
  `http://127.0.0.1:…` and `http://[::1]:…` are allowed;
- keeps `ANTHROPIC_BASE_URL` in its secret-redaction lists, so it is treated as
  a first-class setting rather than an accident.

Caveats measured or visible in the code:

- Sign-in to claude.ai is still required for the app's account/org; the base
  URL changes inference, not authentication.
- The daemon reads the environment at startup. A provider switch means
  `claude-science stop` → `claude-science serve --no-browser --detached` with
  the new environment. There is no live reload.
- Science consumes the model catalog from the endpoint. A local model whose id
  does not look like a Claude model may be filtered or assigned no tier — the
  same class of problem Claude Desktop had (`dd1aaf6`: route ids must be
  `claude-…`, display names carry the real model).

## Recommended shape

Point Science at the **CC Switch gateway**, not at the upstream provider
directly:

```
Claude Science ──ANTHROPIC_BASE_URL──▶ http://127.0.0.1:15723/claude-science
                                        │
                                        └─▶ provider routes (DeepSeek, MiMo, Monet Local, …)
```

Reasons:

- provider switching, failover, model routes and usage logging stay in one
  place — this is what the user asked for ("CC Switch with DeepSeek as the
  replacement for the official Anthropic provider");
- `/v1/models` can publish `claude-*` route ids and hide upstream names,
  which is what Science (like Desktop) expects;
- direct mode (env pointing straight at a provider's `/anthropic` endpoint)
  remains useful as a fallback, but gives up usage and failover.

The existing Desktop machinery is the template:

| Concern | Claude Desktop | Claude Science |
| --- | --- | --- |
| Provider storage | `providers` row, `settings_config.env` | same row shape, same `env` keys |
| Model routes | `meta.claudeDesktopModelRoutes` | reuse the same map |
| Gateway URL | `proxy_gateway_base_url_from_db` + `/claude-desktop` | same, `/claude-science` |
| Gateway token | `get_or_create_gateway_token` | same token (or a per-app one) |
| Apply | write `claude_desktop_config.json` profile | restart daemon with env |
| Mode lifecycle | `mode::{direct,route,stack}` | direct/route/stack, restart on change |

## Touch points found in the fork

Rust (`src-tauri/src`):

- `app_config.rs` — `AppType` enum, `as_str`, `all`, `FromStr`, MCP/prompt
  containers, `is_additive_mode`, `supports_local_proxy`; every exhaustive
  match (the compiler will list them).
- `proxy/server.rs` — route matching for `/claude-desktop/v1/...`; add
  `/claude-science/v1/models` and `/v1/messages`.
- `proxy/handlers.rs` — the `(app_type, some("/claude-desktop"))` table and
  `select_providers("claude-desktop")`.
- `proxy/usage/*` — fold or bucket Science usage under its own app id; the
  Desktop commit did the same (`7ef1f22`).
- `commands/proxy.rs`, `services/provider/mod.rs`, `database/*` — provider
  lifecycle, where `claude-desktop` appears.
- `claude_science.rs` — this branch: env construction, URL validation,
  `status`, `stop`, `serve`; unit tests.

Frontend (`src`):

- `lib/api/types.ts` (`AppId`), `config/appConfig.tsx` (`APP_IDS`,
  `APP_ICON_MAP`, `PROXY_APP_IDS`/Stack), `types/usage.ts`
  (`AppType`, `KNOWN_APP_TYPES`).
- `components/providers/forms/` — reuse `ClaudeDesktopProviderForm` (same env
  + model routes) under the new app id; `presentation.ts`, `ProviderList.tsx`,
  `AddProviderDialog.tsx`.
- i18n labels and the app icon.

## Proposed commits

1. **Foundation** (this branch): `claude_science.rs` + tests + this document.
2. **App type + provider switching (direct mode)**: `AppType::ClaudeScience`,
   provider form/presets, apply = stop/serve with the provider's env.
3. **Gateway route**: `/claude-science` in the proxy, model aliases, usage
   bucket; apply = gateway URL + token.
4. **Mode lifecycle**: route/stack, restart-on-change, status surface in the
   UI ("daemon running on port 8000").
5. **Polish**: icon, i18n, docs, usage import from Science's DB if wanted.

## Test plan

- Unit: `cargo test claude_science` (URL rules, env extraction, args, exe
  resolution). No daemon is started by the tests.
- Manual spike (safe, separate data dir): run Science with
  `--data-dir` pointing at a scratch folder so the real scientific data is not
  touched, then check whether its model picker lists the gateway's aliases and
  whether one short chat completes. The real data dir contains the user's
  research and must not be the first test target.
- End-to-end on the real install only after the spike: switch provider in CC
  Switch → daemon restarts → UI shows a fresh sign-in link → chat uses the
  routed model.

## Warning for development

Our own session runs through the compiled `MultiClaude.exe` in the repository
root, whose gateway listens on 15723. Do **not** replace that exe or start a
second instance on the same port while working: `cargo check/test` is safe,
`pnpm tauri dev` and an in-place rebuild are not.
