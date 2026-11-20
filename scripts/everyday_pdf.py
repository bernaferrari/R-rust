"""Independent rendered PDF checks for everyday analysis."""

import re
import shutil
import subprocess

LABELS = ("Weekly observations", "Observations", "Week", "Observed", "Linear fit")
POINTS = ((59, 183), (145, 152), (230, 167), (316, 105), (401, 90))
AXES = ((60, 214, 400, 214), (59, 59, 59, 214))


def read_pdf_page(path, page=1):
    for tool in ("pdfinfo", "pdftotext", "pdftoppm"):
        if not shutil.which(tool):
            raise ValueError(f"{tool} from Poppler is required for the PDF workflow")
    metadata = subprocess.check_output(["pdfinfo", path], text=True, timeout=30)
    page_count = int(re.search(r"^Pages:\s+(\d+)$", metadata, re.MULTILINE)[1])
    dimensions = list(map(int, re.search(
        r"^Page size:\s+(\d+) x (\d+) pts", metadata, re.MULTILINE).groups()))
    labels = subprocess.check_output(
        ["pdftotext", "-layout", "-f", str(page), "-l", str(page), path, "-"],
        text=True, timeout=30)
    words = " ".join(labels.split())
    header, size, depth, image = subprocess.check_output(
        ["pdftoppm", "-f", str(page), "-l", str(page), "-r", "72", "-singlefile", path], timeout=30
    ).split(b"\n", 3)
    width, height = map(int, size.split())
    if header != b"P6" or depth != b"255" or len(image) != width * height * 3:
        raise ValueError("unexpected Poppler raster format")

    return {"pages": page_count, "page_size_points": dimensions}, words, width, image


def inspect_page(path, expected_labels):
    metadata, words, width, image = read_pdf_page(path)
    labels = [label for label in expected_labels if re.search(rf"\b{re.escape(label)}\b", words)]
    return {**metadata, "labels": labels}, width, image


def color_positions(image, width, color):
    return [(index % width, index // width)
            for index, pixel in enumerate(zip(image[::3], image[1::3], image[2::3]))
            if pixel == color]


def near(position, point, tolerance=4):
    return all(abs(a - b) <= tolerance for a, b in zip(position, point))


def inspect_scatterplot(path):
    page, width, image = inspect_page(path, LABELS)

    def segment_support(pixels, endpoints):
        x0, y0, x1, y1 = endpoints
        return sum(x0 - 1 <= x <= x1 + 1 and y0 - 1 <= y <= y1 + 1 for x, y in pixels)

    navy = color_positions(image, width, (0, 0, 128))
    fit = color_positions(image, width, (178, 34, 34))
    reference = color_positions(image, width, (102, 102, 102))
    ink = color_positions(image, width, (0, 0, 0))
    counts = {
        "data_points": sum(any(near(position, point) for position in navy) for point in POINTS),
        "fit_pixels": sum(65 <= x <= 395 and abs(y - (188 - (x - 60) * 0.29)) <= 5 for x, y in fit),
        "reference_pixels": sum(61 <= x <= 398 and abs(y - 136) <= 1 for x, y in reference),
        "horizontal_axis_pixels": segment_support(ink, AXES[0]),
        "vertical_axis_pixels": segment_support(ink, AXES[1]),
    }
    manifest = {
        **page,
        "data_points_visible": counts["data_points"] == 5,
        "fit_visible": counts["fit_pixels"] >= 200,
        "reference_visible": counts["reference_pixels"] >= 80,
        "axes_visible": counts["horizontal_axis_pixels"] >= 325
        and counts["vertical_axis_pixels"] >= 140,
    }
    return manifest, counts


EXPECTED_SCATTERPLOT = {
    "pages": 1,
    "page_size_points": [432, 288],
    "labels": list(LABELS),
    "data_points_visible": True,
    "fit_visible": True,
    "reference_visible": True,
    "axes_visible": True,
}
