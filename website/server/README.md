# Hosted R drafting

`configuredDraftHandler(env)` returns a Fetch-style request handler for a server
adapter. It is intentionally not enabled or routed by the static website yet.
Hosting, the sign-in provider and the spending cap need to be configured before
adding the editor button and publishing an endpoint.

The client sends `POST` JSON `{ "prompt": "..." }` with a verified sign-in
provider's bearer token. Successful responses contain `{ "code": "..." }`.
Drafts must be reviewed before replacing or running editor contents. The user's
prompt is sent to DeepSeek; existing editor data is not sent automatically.

Required **server-only** configuration:

- `DEEPSEEK_API_KEY`
- `AI_ALLOWED_ORIGIN`: the exact website origin
- `AI_JWKS_URL`, `AI_JWT_ISSUER`, `AI_JWT_AUDIENCE`: the chosen sign-in provider
- `AI_REDIS_URL`, `AI_REDIS_TOKEN`: a shared Redis REST endpoint supporting EVAL
- `AI_DAILY_TOKEN_BUDGET`: positive daily global token reservation ceiling

The server verifies signed, expiring JWTs before reserving quotas. It uses the
issuer/subject identity, not a client-provided user ID. Account limits are three
requests per minute and ten per UTC day; the global ceiling applies across all
accounts. Redis reserves these atomically and fails closed if unavailable.
Failed/cancelled generations keep their reservation. There are no automatic
provider retries. Multi-region replicas must use the same strongly consistent
quota store, not independent counters.

Model: `deepseek-v4-flash`, thinking disabled, maximum 2,048 output tokens and a
20-second generation deadline. Request size is bounded while reading, not just
by trusting Content-Length. The daily token ceiling is conservative accounting,
not a currency guarantee: set a provider spending limit as well and revisit the
conversion whenever pricing changes. Never use VITE-prefixed variables for keys.

Sources:
- [DeepSeek API](https://api-docs.deepseek.com/)
- [AI SDK DeepSeek provider](https://ai-sdk.dev/providers/ai-sdk-providers/deepseek)
