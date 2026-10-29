Pinned GNU R control: source commit
`bac583951b728e97b9786804d3b4081f0fe18df5`, development R r90451 (2026-08-27).

```
Rscript --vanilla contract.R > contract.out
```

The contract is `src/main/summary.c::do_first_min` and, for the canonical list
coercion diagnostic, `src/main/coerce.c::coerceVectorList`. Selection skips
missing values, keeps the first tie, and copies the selected comparison name.
Classed inputs other than dataframes use xtfrm; other nonnumeric inputs coerce
to double. Names come from that working value, not necessarily the source.

The custom xtfrm method is bound in .GlobalEnv so it is actually eligible for
S3 lookup from the base which.min/which.max frame. A method bound only inside
local() is not an equivalent GNU control. GC runs inside both the transform
method and coercion warning handlers. The fixture also covers dataframes,
factor/ordered, Date, numeric edge cases, and direct list/expression errors for
five coercion targets. The same output is required in native and restricted
portable sessions; no normalization is applied.
