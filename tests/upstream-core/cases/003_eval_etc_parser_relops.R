## Curated from r-source/tests/eval-etc.R:
## parser newline continuation around relational operators.
print(1 <
    2)
print(2 <=
    3)
print(4 >=
    3)
print(3 >
    2)
print(2 ==
    2)
print(1 !=
    3)
print(all(NULL == NULL))
