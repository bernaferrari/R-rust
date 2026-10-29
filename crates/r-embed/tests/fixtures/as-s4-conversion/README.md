The output is captured unchanged from GNU R source commit
`bac583951b728e97b9786804d3b4081f0fe18df5` (R Under development,
2026-08-27 r90451), with:

```
Rscript --vanilla conversion.R > conversion.out
```

The public fixture covers complete=0/1/2/-1, basic types, preserved arbitrary
attributes, independent mutation, genuine methods numeric/oldClass/slot-only
classes, and reference-preserving environment data parts. The same fixture is
executed with native and restricted portable runtime paths.

The pinned implementation is `src/main/objects.c` (`asS4`) and
`src/main/attrib.c` (`R_getS4DataSlot`). Complete=0 only clears the marker;
other modes first try a basic S3 data part. Conditional failure preserves the
original object. Successful conversion restores `.S3Class` or removes a formal
class when there is no S3 class, and retains unrelated attributes.
