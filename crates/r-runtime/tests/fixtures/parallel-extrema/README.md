Pinned GNU R source commit bac583951b728e97b9786804d3b4081f0fe18df5,
development r90451 (2026-08-27).

Generate exact output with `Rscript --vanilla contract.R > contract.out`.
The numeric kernel contract is src/main/summary.c::do_pmin. NULL/logical inputs
promote to integer only for multiple inputs; a single input is returned as-is.
The first zero/nonzero length mismatch stops result-type discovery. Missing
values follow the original ordered fold, including distinct NA/NaN payload
selection, and fractional recycling warns once.

Both native and restricted portable sessions require the same exact 80 rows.
The scope is plain logical/integer/real/NULL inputs plus rejection of plain
complex/raw inputs, with first-input names/dimensions and .int distinctions.
Character/classed paths and the existing arbitrary-attribute policy are not
migrated by this numeric slice. Warning handlers force GC; no normalization is
applied to oracle output.
