---
active: true
iteration: 3
completion_promise: "VERIFIED"
initial_completion_promise: "DONE"
verification_attempt_id: "ea8e0a8b-9174-4b36-b1a8-617f9279446a"
started_at: "2026-04-03T05:26:53.216Z"
session_id: "ses_2ae509f95ffez1Kgpl5qwKcerQ"
ultrawork: true
verification_pending: true
strategy: "continue"
message_count_at_start: 122
---
Yes. I think this is a very good idea if the project is framed as a compatibility-first reimplementation of libR, not as “let’s rewrite everything and clean it up later.” The current tree is still the classic split of src/main, src/library, src/nmath, src/appl, src/modules, src/unix, and src/gnuwin32, with graphics backends under grDevices and a large existing tests/ corpus. Also, most user-visible functionality is already written in R and calls a smaller primitive/runtime layer, while the ecosystem still depends heavily on .Call, .External, shared libraries/DLLs, and routine registration. That means the winning move is a Rust runtime behind a stable R-compatible ABI, not a greenfield “R-like” language runtime.  ￼

The hardest parts are not BLAS wrappers. They are SEXP semantics, environments, promises/lazy evaluation, non-local control flow via contexts, write barriers and GC, serialization, ALTREP, S4/S3 dispatch, graphics-device behavior, and extension ABI. R Internals explicitly documents all of those as central internal structures, and contexts still carry a JMP_BUF, promises are central to closure calls, BCODESXP exists for bytecode, S4 objects can be any SEXPTYPE, and ALTREP affects serialization. That is why the architecture has to isolate language semantics from host/platform code.  ￼

The architecture I would actually build

I would build three layers.

First, a compatibility shell. This is the public face: R.h, Rinternals.h, R_ext/*, dynamic loading, .Call, .External, .C, .Fortran, native-routine registration, symbol lookup, and embedding entry points. Current R explicitly documents these interfaces and recommends registered routines with R_registerRoutines, R_useDynamicSymbols(FALSE), and often R_forceSymbols(TRUE). This layer stays boring and stable for years.  ￼

Second, a Rust semantic core. This owns the interpreter session, object model, GC, environments, evaluator, bytecode, serialization, connections, RNG, and math. Internally, this should be idiomatic Rust. Externally, it should still look like libR. Because SEXP is documented as opaque to extensions, you have room to redesign internal layout and safety mechanisms as long as pointer stability and observable behavior remain compatible.  ￼

Third, platform and device hosts. Today’s source tree already separates optional modules like internet, lapack, and X11, plus Unix, Windows, and macOS-specific code. Keep that idea, but make the host surface much thinner: console I/O, dynamic loading, filesystem/process integration, timers, event-loop glue, and graphics surfaces. The language runtime should not know whether it is running in a Unix CLI, Rgui, R.app, or Android app shell.  ￼

The key design principle is: logical layering, physical mirroring. Logically, separate core/ABI/host. Physically, mirror upstream file names and subsystem names as closely as possible so future upstream sync stays tractable.

The file tree I would choose

I would not use a fashionable “clean” tree that loses the upstream map. I would use a Rust workspace whose crate names mirror R’s existing subsystems, and whose Rust file names mirror the current C file names wherever practical.

r-rust/
  Cargo.toml
  rust-toolchain.toml

  crates/
    r-runtime/                # top-level session orchestration, libR equivalent
    r-main/                   # mirrors src/main
      src/
        eval.rs
        envir.rs
        context.rs
        memory.rs
        serialize.rs
        connections.rs
        errors.rs
        dotcode.rs
        duplicate.rs
        altrep.rs
        parser/
        bytecode/
    r-nmath/                  # mirrors src/nmath
    r-appl/                   # mirrors src/appl
    r-modules/                # internet / lapack / optional modules
    r-graphics-engine/        # GE equivalent
    r-grdevices/              # grDevices shared logic, file devices
    r-device-x11-compat/
    r-device-win32-compat/
    r-device-quartz-compat/
    r-device-headless/
    r-platform-unix/
    r-platform-windows/
    r-platform-macos/
    r-platform-android/
    r-embed/                  # embedding APIs and host callbacks
    r-ffi/                    # exported C ABI, headers, shims
    r-library-base/           # compiled pieces for base
    r-library-compiler/
    r-library-graphics/
    r-library-grdevices/
    r-library-methods/
    r-library-parallel/
    r-library-stats/
    r-library-utils/
    r-test-harness/

  compat/
    include/                  # R.h, Rinternals.h, R_ext/*
    c-shims/
      longdouble/
      dynload/
      unwind/
    fortran/
      translated/
      wrappers/

  upstream/
    r-source/                 # subtree/submodule mirror of upstream
    mapping/
      files.toml              # src/main/eval.c -> crates/r-main/src/eval.rs
      symbols.toml            # do_eval -> do_eval
      tests.toml

  tests/
    conformance/
    differential/
    graphics/
    embedding/
    fuzz/
    packages/
    android/

  profiles/
    desktop-compat.toml
    desktop-pure-rust.toml
    android-headless.toml

This shape matches the current repo reality: src/main is the big runtime center, src/nmath and src/appl are nicely separable numerical subsystems, src/modules is already a module boundary, grDevices already splits device code, and the test tree already has Embedding, Examples, Pkgs, and lots of .Rout.save golden outputs.  ￼

Inside r-main, I would keep names like eval.rs, envir.rs, connections.rs, altrep.rs, errors.rs, context.rs, and even exported do_* names where that helps upstream comparison. In the first years of the port, keeping names similar is absolutely the right move. Not because it is elegant, but because it makes review, diffing, and future porting dramatically easier.

The three build profiles I would support

I would make the project explicitly multi-profile from day one.

desktop-compat: maximum behavioral compatibility, package compatibility, native devices, optional external BLAS/LAPACK, optional C long-double shim, full embedding/CLI support. This is the profile used to prove “it is still R.” Current R already has optional lapack/X11 modules and platform-specific device paths, so a feature-matrix build is natural.  ￼

desktop-pure-rust: no internal Fortran, pure-Rust core and numerics where feasible, still allows optional external BLAS/LAPACK for people who want peak linear-algebra performance. This is the profile that drives maintainability and memory-safety wins. It can preserve the public C ABI while replacing internals aggressively.  ￼

android-headless: no desktop devices, no Quartz/X11/Windows UI, no internal Fortran, no on-device compiled-package story at first, file/memory graphics only, Rust-only engine. This is the cleanest mobile target. Rust officially supports Android targets with std, and Android’s own NDK docs explicitly call Rust the best choice for memory-safe native code when native code is needed.  ￼

Fortran: what I would keep, what I would remove

My view is:

For internal R code, Fortran should be transitional, not architectural. src/appl still contains a lot of Fortran today, and R’s LAPACK support can already be internal or external. But R’s own extension manual also documents several portability hazards in C/Fortran interop: compiler-dependent LOGICAL, string-length conventions via FC_LEN_T/FCONE, and especially callbacks from Fortran into C, where the manual explicitly says the most portable solution may be to convert the Fortran code to C. That is a very strong signal that a long-term Rust port should not depend on internal Fortran.  ￼

So I would use a three-backend policy:
	1.	Pure Rust kernels for as much of src/appl and src/nmath as possible.
	2.	Optional external BLAS/LAPACK on desktop/server builds for performance and user expectations.
	3.	Temporary translated/wrapped legacy code only during migration, never as the final architecture.

That means the toggle model you suggested is exactly right, but I would apply it mostly to linear algebra backends, not to the whole interpreter. Think feature flags like:
	•	external-blas-lapack
	•	bundled-reference-lapack
	•	pure-rust-linalg
	•	compat-fortran-interop (temporary, desktop only)

For Android, I would ship no internal Fortran at all.

One more important distinction: I would still preserve the package-facing .Fortran ABI in the compatibility layer for desktop builds, because existing packages use it. I just would not let the interpreter’s own internals depend on it forever. R’s README and extension docs make clear that interfacing to C/Fortran is part of the ecosystem contract.  ￼

How Rust should change the runtime design

Rust gives you far more than “rewrite C safely.” It lets you encode R invariants in the type system.

The most important change is to make the session and GC explicit. Current R uses a non-moving generational collector, R_alloc scratch memory with vmaxget/vmaxset, and write-barrier-sensitive mutation via functions like SET_VECTOR_ELT and SET_STRING_ELT. In Rust, I would model that as:
	•	Session: owns the whole interpreter state; deliberately !Send and !Sync.
	•	Sexp: opaque handle used at ABI boundaries.
	•	Root<T> / Gc<T> / Scope: rooted and scoped handles for internal code.
	•	mutation only through barrier-aware methods such as set_vector_elt.
	•	separate transient scratch arenas for parser/evaluator temp storage instead of ad hoc raw allocation.

That maps directly onto the semantics R already documents, but makes it impossible for internal Rust code to forget a root or bypass a write barrier by accident.  ￼

I would also redesign internal error handling. R contexts still carry JMP_BUF and non-local control state, and the extension docs are very clear that R API calls belong on the main thread. In a mixed C/Rust phase, that means never allowing a legacy longjmp to cross Rust frames. All legacy-C calls that can non-locally exit need C trampolines. Long term, I would move the interpreter’s internal control flow to Result/condition objects/restart stacks in Rust, and only emulate legacy non-local exits at the C ABI boundary. This is one of the single biggest architecture decisions in the whole port.  ￼

ALTREP is another place where Rust is a natural fit. R already tracks ALTREP in object headers and version-3 serialization supports custom ALTREP serialization. In Rust, I would implement ALTREP classes as explicit trait/vtable objects behind safe wrappers, with all fallback materialization paths centralized and fuzz-tested. That is cleaner and safer than scattered macro-heavy C code.  ￼

Bytecode is similar. R already has BCODESXP, bytecode source maps, and a bytecode compiler/interpreter. In Rust, bytecode should become a strongly typed instruction enum plus compact operand arrays and debug maps, rather than loosely coupled structs and macros. That is one of the easiest areas to make the code more readable without changing semantics.  ￼

How I would make it safer

I would set a hard rule: all unsafe lives in a handful of audited crates/modules. Realistically those are r-ffi, r-main::memory_raw, r-main::unwind_boundary, r-device-* ABI glue, and the long-double shim. Everything else should be safe Rust.

Concrete safety wins:
	•	Encode SEXPTYPE as enums/newtypes instead of raw integer tags.
	•	Encode promise state, device lifecycle, connection state, and condition handling as explicit state machines.
	•	Make Session non-thread-shareable by type, matching R’s documented main-thread expectations.
	•	Make vector/list mutation go through barrier-aware APIs only.
	•	Replace nullable/raw pointer conventions with Option, NonNull, and typed handles.
	•	Forbid panics across C/JNI boundaries.
	•	Add Miri for unsafe-heavy test suites and sanitizers for CI/debug builds; both are first-class Rust tooling for UB and memory/thread bugs.  ￼

I would also use Rust to make string/encoding code less error-prone. R serialization version 3 stores native encoding and has to deal with converting unflagged strings when moving across native encodings. That is exactly the kind of place where explicit string wrappers and encoding-state types help.  ￼

long double: what I would do

I would not try to make long double a first-class Rust numeric type across the whole runtime.

Stable Rust exposes C aliases like c_double and c_long, but not a stable c_longdouble alias in std. Rust’s f128 exists only as a nightly experimental API, and Rust’s own compiler docs note that fp128 math can be lowered to long double symbols on platforms where long double is not IEEE binary128. That makes f128 a bad general answer here.  ￼

So I would isolate long-double behavior behind a tiny backend interface:
	•	NoLongDouble backend: use f64; equivalent to a
