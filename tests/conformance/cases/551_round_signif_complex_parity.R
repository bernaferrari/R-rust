# round()/signif() on complex vectors: stock dispatches to complex_math2
# (main/complex.c z_rround/z_prec), rounding real and imaginary parts
# independently — z_rround uses the real path's ties-even fround, z_prec
# (z_prec_r) scales both parts by the shared max magnitude before fround —
# recycling digits per element, preserving attributes, and keeping the
# all-NA input rule.
z <- complex(real = c(0.5, 1.5, 2.5, -0.5), imaginary = c(-0.5, 2.5, -1.5, 3.5))
print(round(z))
print(round(z, 1))
print(signif(z, 1))
print(signif(complex(real = 123.456, imaginary = 0.789), 2))
print(round(complex(real = -2.5, imaginary = -1.5)))
print(round(2.5 + 1.5i))
print(round(z, digits = 0:1))
print(round(structure(z, foo = "bar")))
print(round(NA_complex_))
print(round(complex(real = NA, imaginary = 1)))
print(signif(NA_complex_, 2))
print(round(complex(0), 1))
print(suppressWarnings(round(complex(real = NA, imaginary = 1))))
