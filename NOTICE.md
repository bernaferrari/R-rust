# Notice

Rport is a Rust port of selected GNU R runtime, math, evaluator, package, and
graphics behavior. It is not a wrapper around an installed GNU R process.

This repository includes pinned upstream GNU R tests under
`tests/upstream-r/vendor` for source comparison, conformance, and attribution.
GNU R is distributed under the GNU General Public License; see `COPYING` and the
upstream notices preserved in the source files. Translated or behaviorally
derived Rust modules should keep their upstream source anchors in
`docs/upstream-port-map.tsv`.

The workspace crate metadata uses `GPL-2.0-or-later`. Keep this notice, the
upstream R license text, and source-map documentation with release artifacts.

Third-party Rust, Gradle, Android, and UniFFI dependencies retain their own
licenses. Release consumers should audit dependency licenses for their shipping
context before redistribution.
