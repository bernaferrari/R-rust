# Public serialization flag controls

Generated with pinned GNU R r90451, source commit
`bac583951b728e97b9786804d3b4081f0fe18df5`.

`roundtrip.R` exercises fourteen legitimate node/value cases, plain and S4,
versions 2/3, and ASCII/XDR/native streams. `roundtrip.out` is its unedited GNU
stdout. The explicit `complete=FALSE` conversion checks result/source flag
isolation without depending on a separate default-complete conversion gap.

`generate.R DIRECTORY` generates the independent GNU S4 input bundles for
versions 2/3. These are read by the unchanged Rust reader. Neither roundtrip
identity nor the fixture requires a particular dense/ALTREP representation.

Reproduction with the pinned Rscript:

```
Rscript --vanilla roundtrip.R > roundtrip.out
Rscript --vanilla generate.R .
```

Pinned primary contract: `src/include/Defn.h` defines LEVELS as the full gp word
and S4 as bit 4. `src/main/serialize.c` PackFlags strips CHARSXP cache/hash bits
and the growable bit for vector kinds; it retains that bit for closures.
