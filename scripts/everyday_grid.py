"""Check real grid output by page, label, and measured geometry."""

if __package__:
    from .everyday_pdf import color_positions, read_pdf_page
else:
    from everyday_pdf import color_positions, read_pdf_page


PANEL_LABELS = "Control panel Treatment panel"
PANELS_SOURCE = [[40, 68, 185, 195], [247, 68, 392, 195]]
PANELS_RESIZED = [[53, 85, 247, 243], [329, 85, 523, 243]]
PALETTE = {"panels": [(0, 0, 128), (178, 34, 34)], "absolute": [(255, 0, 0)]}


def figure(size, labels, rectangles=()):
    pages = [{"text": label, "blank": not label, "rectangles": []} for label in labels]
    pages[-1]["rectangles"] = list(rectangles)
    return {"page_size_points": list(size), "pages": pages}


SMALL = (432, 288)
LARGE = (576, 360)
EXPECTED_FIGURES = {
    "plot/panels.pdf": figure(SMALL, [PANEL_LABELS], PANELS_SOURCE),
    "plot/panels-live.pdf": figure(LARGE, [PANEL_LABELS], PANELS_RESIZED),
    "plot/panels-replay.pdf": figure(LARGE, ["", PANEL_LABELS], PANELS_RESIZED),
    "plot/panels-restored.pdf": figure(LARGE, [PANEL_LABELS], PANELS_RESIZED),
    "absolute/source.pdf": figure(SMALL, ["Fixed text"], [[47, 90, 83, 112]]),
    "absolute/replay.pdf": figure(LARGE, ["", "Fixed text"], [[47, 115, 83, 137]]),
    "recording/source.pdf": figure((504, 504), ["After inhibit With recording Before recording"]),
    "recording/with.pdf": figure(LARGE, ["Destination", "With recording"]),
    "recording/without.pdf": figure(LARGE, ["Destination"]),
    "recording/inhibited.pdf": figure(LARGE, ["Destination"]),
    "paging/grob-viewport.pdf": figure(LARGE, ["Marker"]),
    "paging/unrecorded-push.pdf": figure(LARGE, ["", "Marker"]),
    "paging/unrecorded-page.pdf": figure(LARGE, ["Marker"]),
    "paging/late-push.pdf": figure(LARGE, ["Marker", "Marker"]),
}
EXPECTED_SAFETY = {"safety/safety.pdf": figure(LARGE, ["Destination survives Drawing can continue"])}
EXPECTED_GRID = {"figures": EXPECTED_FIGURES, "font_sizes_preserved": True}


def bounding_box(positions):
    if not positions:
        return []
    x, y = zip(*positions)
    return [min(x), min(y), max(x) + 1, max(y) + 1]


def inspect_figure(path, colors):
    metadata, _, _, _ = read_pdf_page(path)
    if not 1 <= metadata["pages"] <= 3:
        raise ValueError(f"unexpected grid page count in {path}")
    pages = []
    text_height = 0
    for index in range(1, metadata["pages"] + 1):
        _, text, width, image = read_pdf_page(path, index)
        blank = all(channel == 255 for channel in image)
        rectangles = [bounding_box(color_positions(image, width, color)) for color in colors]
        pages.append({"text": text, "blank": blank, "rectangles": [box for box in rectangles if box]})
        if index == metadata["pages"] and colors:
            pixels = [(offset % width, offset // width)
                      for offset, (r, g, b) in enumerate(zip(image[::3], image[1::3], image[2::3]))
                      if r == g == b and r < 180]
            ink = bounding_box(pixels)
            text_height = ink[3] - ink[1] if ink else 0
    return {"page_size_points": metadata["page_size_points"], "pages": pages}, text_height


def inspect_grid(directory):
    figures, heights = {}, {}
    for name in EXPECTED_FIGURES:
        group = name.split("/", 1)[0]
        colors = PALETTE["panels"] if group == "plot" else PALETTE.get(group, [])
        figures[name], heights[name] = inspect_figure(directory / name, colors)
    pairs = [(f"plot/panels{suffix}.pdf", "plot/panels.pdf")
             for suffix in ("-live", "-replay", "-restored")]
    pairs.append(("absolute/replay.pdf", "absolute/source.pdf"))
    stable = all(5 <= heights[source] <= 16 and abs(heights[replay] - heights[source]) <= 1
                 for replay, source in pairs)
    return {"figures": figures, "font_sizes_preserved": stable}
