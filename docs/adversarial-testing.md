# Adversarial Safety Checks

The release gate already runs these tests through `cargo test -p rmath`, but
the focused script is useful while changing parser, evaluator, namespace, SEXP,
or Android input paths.

Short mode:

```bash
scripts/adversarial_safety_checks.sh --check
```

Longer deterministic mode:

```bash
scripts/adversarial_safety_checks.sh --long
```

Custom iteration count:

```bash
scripts/adversarial_safety_checks.sh --iterations 10000
```

Coverage today:

- parser inputs generated from R syntax punctuation, quotes, comments, and
  malformed delimiters must return `Ok`/`Err`, not panic
- NAMESPACE directive parsing handles comments, strings, nested calls, malformed
  calls, `export`, `exportPattern`, `import`, `importFrom`, `S3method`, and
  `useDynLib`
- selected evaluator and subset errors must stay contained as R errors
- owned `RValue` conversion for generated numeric and string vectors must keep
  length and type shape

The generator is deterministic and uses `RPORT_ADVERSARIAL_ITERS` to scale work.
It requires no network, emulator, or corpus download.
