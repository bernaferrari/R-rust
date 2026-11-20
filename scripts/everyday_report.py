"""Check the forecast report and its two rendered plot states."""

import hashlib

if __package__:
    from .everyday_pdf import color_positions, inspect_page, near
else:
    from everyday_pdf import color_positions, inspect_page, near

PHASES = ('initial', 'rebuild')
OUTPUTS = ('index.html', 'forecast.csv', 'field-trial.pdf')
LABELS = ('Field trial', 'Manure applied', 'Crop yield')
PLOTS = {
    'initial': {
        'points': ((194, 172, 165, 144), (165, 144, 151, 115)),
        'fit': ((197, 140, 121), (172, 115, 96)),
    },
    'rebuild': {
        'points': ((194, 172, 165, 144), (165, 144, 151, 72)),
        'fit': ((209, 127, 101), (173, 91, 65)),
    },
}
FORECASTS = {'initial': ['16.75', '20.25'], 'rebuild': ['19', '24']}
EXPECTED_FIGURE = {
    'pages': 1,
    'page_size_points': [432, 288],
    'labels': list(LABELS),
    'visible_points': [4, 4],
    'visible_fit_samples': [3, 3],
}


def inspect_figure(path, phase):
    page, width, image = inspect_page(path, LABELS)
    controls = PLOTS[phase]
    points, fits = [], []
    for group, color in enumerate(((0, 0, 128), (178, 34, 34))):
        pixels = color_positions(image, width, color)
        # Filled markers occupy a neighborhood around each measured point.
        centers = zip((124, 177, 230, 283), controls['points'][group])
        points.append(sum(sum(near(p, center, 3) for p in pixels) >= 12 for center in centers))
        # Sample both fitted lines away from the original observations.
        samples = zip((105, 305, 370), controls['fit'][group])
        fits.append(sum(any(near(p, sample, 3) for p in pixels) for sample in samples))
    return (dict(page, visible_points=points, visible_fit_samples=fits),
            hashlib.sha256(image).hexdigest())
