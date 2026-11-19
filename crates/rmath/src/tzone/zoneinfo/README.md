# Wasm IANA timezone data

This bundle uses the public-domain IANA tzdb `2026c` data distributed in
`src/extra/tzone/zoneinfo.zip` at the GNU R oracle revision
`bac583951b728e97b9786804d3b4081f0fe18df5`. The index maps 597 names, including
aliases, to 340 unique compressed TZif payloads. Only the Wasm runtime embeds
these assets. [IANA license](https://data.iana.org/time-zones/tzdb/LICENSE)

To regenerate the assets from a local copy of the pinned source archive:

```bash
python3 scripts/vendor_wasm_zoneinfo.py /path/to/zoneinfo.zip
```

The generator checks the archive checksum before writing the index and binary.
