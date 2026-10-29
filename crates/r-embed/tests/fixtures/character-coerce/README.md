Character coercion GNU controls

contract.R contains 64 public GNU controls, including digits settings, special values, metadata isolation and method-dispatch neighbors. contract.out was produced by pinned GNU Rscript 4.7.0 development r90451. Both baseline and candidate pass these public value controls.

The independent ALTREP callback probe in the evidence directory observes getOption("digits") during GNU character conversion: every callback sees 3 while conversion still formats to 15 significant digits. The baseline Rust lazy provider sees 15 instead. The owner test also demonstrates foreign integer source acceptance on baseline. No baseline undefined behavior or destructor abort is claimed.

Additional pinned ALTREP callback controls prove scipen is captured at entry, OutDec remains live, and nested printing or nested character conversion resets precision to option digits. The active-scope flag confines this behavior to character coercion. Outermost cleanup restores the internal prior format sentinel rather than leaving a positive precision that would freeze future options. Nested cleanup restores entry-option digits while keeping the enclosing conversion active.
