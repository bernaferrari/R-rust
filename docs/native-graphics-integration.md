# Native graphics bridge execution evidence

The graphics registry retains typed `.Call`, `.External`, and `.External2`
descriptors and their argument-count admission. Registration alone is not
evidence of drawing support. The following describes the bridge after the
native raster/text integration milestone, under `renderplot-device` with an
active owned scene and the faer numerical backend.

| Native entry | Public wrapper / implementation | Observed bridge behavior |
| --- | --- | --- |
| `C_raster` | `graphics::rasterImage`; shared portable raster decoder and scene image drawing | Connected. GNU positional payload, recycled placements, interpolation flag, transforms and actual RGBA pixels are exercised. Invalid dimensions/colors fail and the session recovers. |
| `C_text` | `graphics::text.default`; portable text and plotmath drawing | Connected. GNU `xy.coords` list is expanded through owned arguments; expression labels, superscripts and colors reach scene commands. |
| `C_arrows`, `C_segments`, `C_rect`, `C_box` | Existing portable drawing routes | Connected previously; original public geometry tests remain the execution evidence. This milestone does not establish every native option. |
| `C_abline` | `graphics::abline`; owned positional adapter to portable drawing | All eight unnamed GNU native fields are routed. Original log-space endpoint, untransformed-curve, and condition/recovery tests pass. Horizontal/vertical color, width and line-type drawing matches public segments under both constructor policies. |
| `C_title`, `C_polygon`, `C_plotXY` | Positional adapters to portable drawing | Connected previously. Native text/raster integration preserves these routes. |
| `C_mtext` | `graphics::mtext`; owned margin arguments, shared fonts/plotmath and scene drawing | Connected. Both public constructor policies pass 61 GNU argument/style/error probes, 32 independently captured PDF coordinate combinations, and three base-character-expansion placements. Missing text emits no glyph; errors recover. Production Wasm repeats all 61 probes in two fresh sessions with real PNGs. |
| `C_filledcontour`, `C_persp` | Legacy routines in `library/graphics/plot3d.rs` | Partial legacy implementation; not admitted through this bridge. Device drawing and buffer contracts still need validation. Requests now raise a named unsupported-operation error. |
| `C_convertX`, `C_convertY` | Legacy routines in `library/graphics/plot.rs` | Existing conversion code is not connected to the owned renderplot coordinate/device contract. Requests now fail explicitly. |
| `C_clip` | Portable per-operation `xpd` clipping exists | Persistent native clipping is not connected. Requests now fail explicitly; `xpd` support is not evidence of native `clip()` support. |
| `C_symbols`, `C_path`, `C_xspline`, `C_dend`, `C_dendwindow`, `C_erase` | No validated scene route for the native payload | Missing bridge operations now fail explicitly instead of returning `NULL` as if drawing succeeded. |
| `C_locator`, `C_identify` | Interactive device operations | No validated interaction provider; explicit unsupported-operation errors. |

The unimplemented entries remain on the full graphics roadmap in
`rport-wpdk.5`. Explicit errors are an interim correctness repair, not completion
of those operations. Without an active renderplot device, the connected text
and raster providers report the required device explicitly. Margin text also
requires that profile and an established plot window.

GNU `as.raster()` and `nativeRaster` store image rows consecutively. Ordinary
unclassed R matrices remain column-major in the public portable decoder. The
native raster payload always uses row order. This distinction is checked with
exact scene pixels and PNG quadrant/rotation assertions, rather than merely
checking that a PNG exists. An independently executed pinned GNU R 4.7
`C_raster` PDF image stream confirmed the row order.

Both real `RSession::new()` and the constructor with an explicit empty library
search policy exercise direct typed native raster/text calls. The portable
`graphics` namespace now supplies qualified public wrappers. The margin-text
acceptance uses those wrappers and the attached unqualified call in fresh
sessions without host package discovery or global helper injection.

Margin placement uses physical `mar`/`oma` line units and captures base `cex` at
`plot.new()`. Margin text's own `cex` remains absolute. The host recording adapter
forwards both text advance and glyph metrics. The exact coordinate oracle uses
the real GNU Helvetica PDF metrics through a matching public `DrawTarget`; PNG
rendering uses the portable DejaVu font. This is not a claim of identical glyph
pixels between those fonts. The portable device retains its thumbnail policy
for default margins at 160-by-120 or smaller viewports, separately from the
504-by-504 physical-device oracle. Explicit margins retain their error rules.

The independent zero-margin GNU capture confirms that plot and figure coincide
when `mar=0`; `xpd` cannot expose more ink in that setup. The clipping regression
now uses positive margins and a rotated image that crosses the plot boundary.
Both its GNU PDF clip/transform commands and its portable PNG ink comparison
are retained. Title containment follows the physical upper margin rather than
the previous fixed pixel boundary. Symbol/Hershey device fonts and the remaining
physical graphics-parameter lifecycle are still separate implementation gaps.

The final native margin batch records 75 passes across 17 suites, including the
existing 54 package/namespace checks. The original title and new margin inputs
survive collection with their argument graph as the only label root. Core build,
warnings-denied Clippy and formatting pass. This selected evidence does not
replace a complete workspace result; the broader run exposed barplot,
log-abline and LOESS failures. Raw evidence is retained under
`target/integration-repair-evidence/mtext-public`.

The subsequent [abline/barplot receipt](ci-checkpoints/native-abline-barplot-358c80f6.json)
records 20 passing selected tests across five suites. The native abline bridge
now preserves the unnamed `h`, `v`, `untf`, `col`, `lty` and `lwd` fields.
Independently executed GNU R returns a 3-by-1 matrix for the vector-height
barplot example; the stale vector expectation was corrected without dropping
dimensions. Real red bars are checked in the original rendering workflow.
Warnings-denied Clippy and formatting pass. LOESS and full workspace completion
remain separate work.

The unchanged whole `reg-tests-1a.R` driver completed at `e312f1b7` under the
explicit graphics/native/faer release profile. GNU exited successfully; the
Rust runner passed margin text and stopped at the named unsupported `C_persp`
operation. The full driver remains failing. Its authenticated producer is
retained under `target/integration-repair-evidence/upstream-reg1a-e312f1b7`;
the perspective bridge is tracked by `rport-t5xt7`.

Validation at the milestone: all 11 existing plotmath tests, all 3 existing
raster tests, and all 3 native bridge contracts pass, also with `vello-gpu`
enabled. The core `--no-default-features --features rust-backend` build passes.
Warnings-denied Clippy and workspace formatting pass. Raw evidence is retained
under `target/native-graphics-integration-evidence` with its source commit and
SHA-256 manifest. The full workspace run is a separate, incomplete check and
has exposed recording-validation failures requiring follow-up.
