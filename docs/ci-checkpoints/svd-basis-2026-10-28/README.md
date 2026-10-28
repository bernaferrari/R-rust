# SVD basis comparison

`input.R` compares rectangular Hilbert, distinct, rank-deficient, repeated,
and zero spectra. `gnu.txt` was produced by the pinned GNU R 4.7 development
oracle (r90451, oracle commit `bac583951b728e97b9786804d3b4081f0fe18df5`)
using its bundled BLAS. `rust.txt` was produced by the clean Rport runner from
commit `77954f8e` using the faer backend. Both commands exited successfully.
Trailing line whitespace was removed from the captured output.

The Hilbert outputs choose opposite signs for two singular-vector pairs. The
rank-deficient output also illustrates that a null-space basis is not unique.
These differences change exact printed output; they do not by themselves show an
incorrect decomposition. Across the six cases, the largest absolute
reconstruction error was 5.33e-15 for GNU and 4.45e-15 for Rport; the largest
orthogonality error was 1.34e-15 for GNU and 8.89e-16 for Rport.

These results describe this oracle and backend, not every platform or input.
Do not canonicalize signs or bases from this single comparison. The separate
whole-driver output mismatch remains tracked independently of these invariant
checks.
