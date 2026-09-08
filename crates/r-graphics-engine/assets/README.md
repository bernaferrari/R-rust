# Bundled plot font

DejaVu Sans is the deterministic default on all platforms, including Wasm.
Its Latin, Greek and mathematical operators support portable plotmath labels.
The unmodified font was copied from the workspace's Poppler font distribution;
its upstream project is https://dejavu-fonts.github.io/.

DejaVu-LICENSE.txt includes the Bitstream Vera and Arev notices from the
upstream version_2_37 tag. DejaVu changes are public domain. The renderer's
synthetic bold and italic transformations do not modify the bundled font file.

Hosts can opt into different fonts through the renderer's set_font API.
