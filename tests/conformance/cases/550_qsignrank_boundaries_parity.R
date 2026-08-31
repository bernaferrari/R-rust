# qsignrank boundary handling: R_Q_P01_check only validates the range, so
# the R_DT_0/R_DT_1 boundary returns are not preempted. p=0 in log space
# (lower.tail=TRUE) is log(1) -> n(n+1)/2, not +Inf; -Inf is rejected by the
# finiteness check with "NaNs produced" from the wrapper.
print(qsignrank(0, 5))
print(qsignrank(0, 5, log.p = TRUE))
print(qsignrank(0, 5, lower.tail = FALSE, log.p = TRUE))
print(qsignrank(1, 5))
print(qsignrank(1, 5, lower.tail = FALSE))
print(qsignrank(1, 5, log.p = TRUE))
print(qsignrank(0:5, 4))
print(qsignrank(0:5, 4, log.p = TRUE))
print(qsignrank(c(-1, 0, 0.5, 1, 2), 6))
# CDF/quantile round trip over the full support
print(qsignrank(psignrank(0:15, 5), 5))
print(qsignrank(psignrank(0:15, 5, lower.tail = FALSE), 5, lower.tail = FALSE))
# Non-finite / out-of-range inputs produce NaN (+ wrapper warning at exit)
print(qsignrank(-Inf, 5))
print(qsignrank(Inf, 5))
print(qsignrank(-1, 5))
print(qsignrank(2, 5))
