# Bundled plot font

DejaVu Sans is the deterministic default on all platforms, including Wasm.
Regular, bold, oblique, and bold-oblique TrueType faces are bundled from the
same upstream 2.37 release, so CPU, GPU, grid, and math layout use matching
glyph coverage and face-specific advances. Its Latin, Greek and mathematical
operators support portable plotmath labels.
The family is from https://github.com/dejavu-fonts/dejavu-fonts/releases/tag/version_2_37.
The unmodified font was copied from the workspace's Poppler font distribution;
its upstream project is https://dejavu-fonts.github.io/.

DejaVu-LICENSE.txt includes the Bitstream Vera and Arev notices from the
upstream version_2_37 tag. DejaVu changes are public domain.

Hosts can opt into different fonts through the renderer's set_font API.
