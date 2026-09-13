cat(deparse(asOneSidedFormula("x")), "\n", sep = "")
cat(deparse(asOneSidedFormula(~x)), "\n", sep = "")
cat(class(asOneSidedFormula("x")), "\n", sep = "")
