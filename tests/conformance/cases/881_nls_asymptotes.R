xy <- sortedXyData(1:8, SSlogis(1:8, 10, 4, 1.5))
cat(round(NLSstLfAsymptote(xy), 4), "\n", sep = "")
cat(round(NLSstRtAsymptote(xy), 4), "\n", sep = "")
