# R_jumpctxt / on.exit — remaining known gaps (Wave 1.2)

## Landed in this branch

- `R_run_onexits_until(target)` walks intervening contexts (cend + conexit) before a jump.
- `R_jumpctxt(target, mask, val)` raises typed `RSignal::{Break,Next,Return,Jump}` instead of `RError("jump_to_context")`.
- `findcontext_jump` wires `break` / `next` / `return` through loop/function contexts.
- `for` / `while` / `repeat` push `CTXT_LOOP` so break/next can target them.
- Unit tests cover nested on.exit + return/break/next and tryCatch interactions.
- Targeted restart transfers unwind intervening functions before the matching handler runs, root arguments across cleanup/GC, and preserve quoted argument data. Nine embedding regressions cover visibility, sibling ordering, nested transfers, warning interception and error recovery; a browser contract exercises cleanup ordering.

## Remaining known gaps

1. **`R_InsertRestartHandlers`** — still a no-op stub in `eval/context.rs`. Interactive defaults (`abort`, `browser`, `tryRestart`) are incomplete versus GNU.
2. **`withRestarts` / `findRestart` / `invokeRestart`** — targeted transfers and function-handler dynamic extents are tested in `essentials/conditions.rs`. Restart specification objects, interactive handlers, and error-path auto-restarts still need full GNU coverage.
3. **Intermediate `jumptarget` longjmp dance** — GNU may longjmp to an intermediate context with `on.exit`, run handlers in `endcontext`, then continue. This port runs *all* intervening handlers up-front before the typed signal (handler stack and protection timing can differ; this is not full GNU semantics).
4. **`CTXT_UNWIND` / `withCallingHandlers` exiting-handler stack** — `ExitingHandler` signal exists; full handler-stack unwind parity with GNU `R_JumpToContext` is incomplete.
5. **ctxt_flags encoding** — this port’s `ctxt_flags::*` values differ from GNU Defn.h; jump masks use separate `JUMP_BREAK`/`JUMP_NEXT` constants.
6. **Bytecode `STARTLOOPCNTXT`** — AST loops push `CTXT_LOOP`; compiled-loop context push/jump parity may still diverge for some tryCatch+break BC paths.
