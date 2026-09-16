cat(round(fft(1:2), 8), "\n", sep = " ")
z <- outer(-1:1 + 0i, 0:1, "^")
cat(Re(z[2, 1]), "\n", sep = "")
