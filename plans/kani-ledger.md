# Kani ledger

Toolchain: Kani 0.68.0 on `nightly-2026-08-21`, invoked by `scripts/cargo_kani.sh`. The workspace pin stays Rust 1.96.0. Unwinding assertions stay on. Statuses below are the harness result, not a percentage of the interpreter.

| Harness | Domain | Bounds | Assumptions | Status |
| --- | --- | --- | --- | --- |
| `kani_trivial_ok` | arithmetic smoke test | none | none | full machine check |
| `vector_length_fits_rejects` | vector-length admission | symbolic `i32` length, remaining `0..=64`, element sizes `{0,1,4,8,16}` | decision enum, no formatted strings | bounded |
| `element_slot_decision_complete` | checked string/list slot | tags `0..=32`, length and index `-2..=4` | `INTEGER_ELT` stays unchecked | bounded |
| `root_table_exhaustion_is_atomic` | generation reservation | `u64::MAX` | calls `try_reserve_generation`, not `catch_unwind` | full for that counter |
| `root_table_ops_preserve_invariant` | claim, release, reuse, stale release | concrete transitions | managed set is a `Vec` | bounded scenario |
| `root_table_restore_keeps_managed` | scope restore | one checkpoint | raw protect roots do not survive | bounded scenario |
| `root_table_legacy_stack_is_disjoint` | legacy `UNPROTECT` | concrete stack | does not touch the root table | bounded scenario |
| `validate_gnu_stream_partition` | GNU bytecode framing | stream length `0..=12` | compared with an independent walk | bounded |
| `gnu_next_pc_matches_width_table` | opcode step shared by the validator and executor | opcodes `0..129`, pc `<= 8`, length `<= 16` | width table is the spec | bounded |
| `argmatch_spec` | three-pass argument match | 2 formals, 2 supplied, name ids `0` and `1` | prefix table checked against `str::starts_with` by a unit test | bounded |
| `child_mask_matches_roles` | collector child slots | type codes `0..=32` | weak key is unmarked and still forwarded on update | bounded |
| `tiny_mark_is_the_reachable_set` | mark fixpoint on an abstract graph | 4 nodes, 3 slots | edges are indexes, not live `SEXP`s | bounded |
| `qr_scratch_rejects_overflow` | real QR scratch admission | dimensions `0..=4`, plus `usize::MAX` | `None` on overflow | bounded, with concrete extremes |
| `svd_scratch_rejects_overflow` | real SVD scratch admission | length `<= 8`, `min(n,p) <= 4`, plus `usize::MAX` | `None` on overflow | bounded, with concrete extremes |
| `loess_workspace_rejects_overflow` | LOESS workspace admission | `n <= 3`, `d <= 2`, queries `<= 3`, plus `usize::MAX` | `None` on overflow | bounded, with concrete extremes |

Not established here: `RSession::eval`, `RCNTXT` / `tryCatch`, live `gengc` pointer tracing, `INTEGER_ELT` bounds, and faer or LOESS numeric error. Those stay on Miri, GC torture, fuzz, and the GNU R oracle.

Mutants to apply locally and not commit: invert `end > len` in `gnu_next_pc`; treat `index == length` as a valid element slot; run the partial pass before the exact pass in `match_states`. Each of those must make its harness fail.
