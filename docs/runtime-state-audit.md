# Runtime State Audit

This audit tracks the remaining non-session-shaped runtime state after the
RInstance/RSession split.

## Session-Owned State

These families live in `RInstance` and are selected through explicit
`RSession` activation. The thread-local current-instance pointer is a
compatibility dispatch slot, not the owner.

| Family | Owner | Notes |
| --- | --- | --- |
| Arena and protect stacks | `RInstance` | Accessed through `RSession::with_arena`, protection APIs, and scoped activation. |
| Environments and symbols | `RInstance` | `RSession::sexp` rejects foreign arena values before evaluation or binding. |
| Eval control | `RInstance::eval_state` | Visibility, depth, limits, profiling, bytecode stack, and cancellation are per session. |
| Output capture | `RInstance` | Capture buffers are restored per session and nested per session. |
| RNG state | `RInstance` | `RSession::unif_rand`, `set_seed`, and `norm_rand` activate the owning session. |
| Android runtime paths | `RInstance::path_policy` | Library and temp directories are configured per session. |

## Explicit Facades Added

The evaluator still has C-shaped helpers such as `get_eval_limits()` for
translated code, but Rust and Android callers now have owner-checked session
facades:

| API | Purpose |
| --- | --- |
| `RSession::eval_limits` | Read limits from this session without relying on whichever instance is current. |
| `RSession::set_eval_limits` | Configure limits on this session only. |
| `RSession::reset_eval_limits` | Restore this session's defaults without touching other sessions. |
| `RSession::replace_cancellation_flag` | Scope a cooperative cancellation token to one owner session. |
| `android::RSession::eval_with_cancellation_flag` | Android embedding path that restores the previous flag after each eval. |

## Compatibility State Still Allowed

| State | Policy |
| --- | --- |
| `sexp::instance::CURRENT_INSTANCE` | Thread-local compatibility dispatch. Keep it small and push new Rust APIs toward explicit `RSession` ownership. |
| Immutable R sentinels | Process-wide immutable values only. Mutable environments are session-owned. |
| Panic hook install guard | Process-wide Rust runtime behavior; not interpreter data. |
| Android global allowlist entries | Tracked in `docs/android-platform-global-allowlist.tsv` and checked by `scripts/check_android_globals.sh`. |

## Current Confidence

The Android-facing eval path can run multiple independent sessions on separate
worker threads. Cancellation tokens still use `Arc<AtomicBool>` because they are
the host-to-worker signal; the atomic is not shared interpreter state and does
not couple sessions.
