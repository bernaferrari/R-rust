Same-type atomic coercion compatibility helpers

The 108-row pinned GNU fixture checks values, metadata stripping and mutation isolation for attributed and plain inputs of all six atomic vector types. Complex vectors are projected to real/imaginary values plus attributes to avoid the separately recorded dput complex-NA spelling discrepancy rport-o7oup; its original failing fixture/log remain in the evidence directory.

Ordinary integer/logical/double/character and as.vector dispatch use essentials_basic::coerce_to_type. Raw and complex S3 wrappers in essentials/s3.rs delegate their default paths to as_atomic_safe, so those public fixture cases exercise this shared helper. Other ordinary routes provide behavior/metadata/isolation neighbor coverage. The demonstrated baseline owner failures are direct checked compatibility APIs, rather than foreign domains expressible in ordinary R.

Entry admission also checks original source and mode allocation generations before classification or mode reads, retaining physical pins and roots across callbacks. Foreign modes, mode callbacks before source rejection, revoked arguments, actual stale physical-slot reuse, and mode-provider typed errors are covered.

Owner unit tests call those actual helpers, covering all six foreign cases, all atomic provider kinds including rooted strings, GC/replacement sessions, bidirectional source/result mutation, typed budget denial before provider reads, same-input recovery with exact resident/node/workspace rollback after error/panic/reentry, and callback revocation. No baseline undefined behavior is claimed.

Only same-type atomic source paths are changed. Scalar coercion, deeper other mode/type branches remain outside this slice; parent rport-mwzqv stays open.

Mode parsing retains prior UTF-8/C-string error semantics using a borrowed pure literal match, ending that borrow before runtime checks. A 256 KiB mode and invalid UTF-8/native-byte controls execute under the same resident/node cap with exact ledgers and valid-mode recovery. Native embedded-NUL prefix coverage preserves this helper's legacy CStr behavior; it is not a claim that GNU accepts embedded-NUL R character construction. A separate pinned GNU ALTREP mode probe confirms provider error propagation; full mode-evaluation multiplicity and any/non-atomic semantics are not claimed audited.

The mode-budget fixture retains 256 KiB for native tests and uses 4 KiB under Miri to bound interpreter byte-validity checking. Budget limits and all invalid UTF-8/Unicode/empty/NUL cases are identical. The original 256 KiB Miri attempt was interrupted after over 30 minutes and remains incomplete; no pass is claimed from it.
