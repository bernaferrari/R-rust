cat(round(fft(1:2), 8), "\n", sep = " ")
z <- outer(-1:1 + 0i, 0:1, "^")
cat(Re(z[2, 1]), "\n", sep = "")
cat(1 / 3 + 0i, "\n", sep = "")
cat(complex(real = 9, imaginary = -0), "\n", sep = "")
cat(as.raw(c(1L, 255L)), "\n", sep = " ")

