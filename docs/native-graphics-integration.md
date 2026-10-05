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
| `C_arrows`, `C_segments`, `C_rect`, `C_box`, `C_abline` | Existing portable drawing routes | Connected previously; original public geometry tests remain the execution evidence. This milestone does not establish every native option. |
| `C_title`, `C_polygon`, `C_plotXY` | Positional adapters to portable drawing | Connected previously. Native text/raster integration preserves these routes. |
| `C_filledcontour`, `C_persp` | Legacy routines in `library/graphics/plot3d.rs` | Partial legacy implementation; not admitted through this bridge. Device drawing and buffer contracts still need validation. Requests now raise a named unsupported-operation error. |
| `C_convertX`, `C_convertY` | Legacy routines in `library/graphics/plot.rs` | Existing conversion code is not connected to the owned renderplot coordinate/device contract. Requests now fail explicitly. |
| `C_clip` | Portable per-operation `xpd` clipping exists | Persistent native clipping is not connected. Requests now fail explicitly; `xpd` support is not evidence of native `clip()` support. |
| `C_mtext`, `C_symbols`, `C_path`, `C_xspline`, `C_dend`, `C_dendwindow`, `C_erase` | No validated scene route for the native payload | Missing bridge operations now fail explicitly instead of returning `NULL` as if drawing succeeded. |
| `C_locator`, `C_identify` | Interactive device operations | No validated interaction provider; explicit unsupported-operation errors. |

The unimplemented entries remain on the full graphics roadmap in
`rport-wpdk.5`. Explicit errors are an interim correctness repair, not completion
of those operations. Without an active renderplot device, the connected text
and raster providers report the required device explicitly.

GNU `as.raster()` and `nativeRaster` store image rows consecutively. Ordinary
unclassed R matrices remain column-major in the public portable decoder. The
native raster payload always uses row order. This distinction is checked with
exact scene pixels and PNG quadrant/rotation assertions, rather than merely
checking that a PNG exists. An independently executed pinned GNU R 4.7
`C_raster` PDF image stream confirmed the row order.

Both real `RSession::new()` and the constructor with an explicit empty library
search policy exercise direct typed native raster/text calls. The portable
`graphics` namespace is not yet available for qualified `graphics:::` calls;
these tests do not claim otherwise or enable host package discovery.

Validation at the milestone: all 11 existing plotmath tests, all 3 existing
raster tests, and all 3 native bridge contracts pass, also with `vello-gpu`
enabled. The core `--no-default-features --features rust-backend` build passes.
Warnings-denied Clippy and workspace formatting pass. Raw evidence is retained
under `target/native-graphics-integration-evidence` with its source commit and
SHA-256 manifest. The full workspace run is a separate, incomplete check and
has exposed recording-validation failures requiring follow-up.
