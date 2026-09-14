xy <- sortedXyData(1:8, SSlogis(1:8, 10, 4, 1.5))
cat(round(NLSstClosestX(xy, 5), 4), "\n", sep = "")
cat(round(NLSstClosestX(xy, 6), 6), "\n", sep = "")
