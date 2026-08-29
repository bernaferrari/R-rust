# Hex numeric constants and string->double parsing (trunk NumericValue):
# bare hex is double; L forces integer when the value is integral; hex
# fractions need no binary exponent; i suffix makes complex; R has no octal
# source literals.
print(0x10)
print(typeof(0x10))
print(0x1p3)
print(typeof(0x1p3))
print(0x1.8)
print(typeof(0x1.8))
print(0x.8p1)
print(0x1.)
print(0x1P-3)
print(0x7fffffff)
print(0x1e5)
print(010)
print(typeof(010))
print(010L)
print(0x10L)
print(typeof(0x10L))
print(0x1p3L)
print(typeof(0x1p3L))
print(0x.8p1L)
print(0x10i)
print(0x1p3i)
print(typeof(0x1p3i))
# String -> double coercions accept hex floats (R_strtod family).
print(as.numeric("0x1p3"))
print(as.numeric("0x1.8"))
# type.convert: hex strings convert to double, garbage stays character.
print(type.convert("0x10", as.is = TRUE))
print(typeof(type.convert("0x10", as.is = TRUE)))
print(type.convert("0x1p3", as.is = TRUE))
print(type.convert(" 42 ", as.is = TRUE))
print(typeof(type.convert(" 42 ", as.is = TRUE)))
print(type.convert("12x", as.is = TRUE))
# dhyper: out-of-range x returns the zero density; R_D_nonint_check in
# dpq.h makes non-integer x return R_D__0 (-Inf in log space) after the
# warning (warning itself not exercised here).
print(dhyper(-0.5, 10, 10, 5))
print(dhyper(2, 10, 10, 5))
print(dhyper(2, 10, 10, 5, log = TRUE))
