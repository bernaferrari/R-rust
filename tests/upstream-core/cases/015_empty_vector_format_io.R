## Curated from r-source/tests/print-tests.R, reg-IO2.R, and reg-tests-1b.R:
## empty-vector formatting, conversion, and platform/path helper invariants.
print(format(character(0)))
print(format(numeric(0)))
print(format.info(character(0)))
print(noquote(character(0)))
print(nchar(character(0)))
print(nzchar(character(0)))
print(file.exists(character(0)))
print(dir.exists(character(0)))
print(file.create(character(0)))
print(basename(character(0)))
print(dirname(character(0)))
