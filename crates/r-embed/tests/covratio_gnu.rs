use r_embed::RSession;

/// GNU R 4.6.1 oracle for `covratio(lm(mpg ~ wt + hp, data=mtcars))`.
/// Doubles are `dput` output from that R, not a re-derived formula.
/// `lm()` here is usable: the stats closure matches those doubles.
#[test]
fn covratio_matches_gnu_oracle() {
    let mut session = RSession::new().expect("session");
    let out = session
        .eval(
            r#"
            near <- function(got, exp, label) {
              got <- as.numeric(got)
              if (length(got) != length(exp)) stop(paste(label, "len", length(got), length(exp)))
              for (i in seq_len(length(exp))) {
                a <- got[[i]]
                b <- exp[[i]]
                if (is.nan(b)) {
                  if (!is.nan(a)) stop(paste(label, "nan", i, a))
                } else if (is.infinite(b)) {
                  if (!identical(a, b)) stop(paste(label, "inf", i, a, b))
                } else if (!is.finite(a) || abs(a - b) > 1e-8) {
                  stop(paste(label, "diff", i, a, b, abs(a - b)))
                }
              }
              TRUE
            }
            run <- function(label, expr) {
              tryCatch(expr, error=function(e) paste(label, "ERR", conditionMessage(e)))
            }

            fit <- lm(mpg ~ wt + hp, data=mtcars)
            cr <- covratio(fit)
            expected <- c(1.04303730954591, 1.11197516466917, 1.06750970889065, 1.16604607726619,
              1.15098025821712, 1.08372768147286, 1.22162448034606, 1.20696169975472,
              1.16942160263852, 1.1543068219349, 1.07860516824836, 1.168456969743,
              1.15757982314696, 1.11040481124057, 1.3644478256944, 1.37535253780841,
              0.722665308328846, 0.647648859338367, 1.24067727287683, 0.643380489886664,
              1.00090577614009, 0.962172572957545, 0.888099097102707, 1.20557569360369,
              1.05459961438794, 1.22171278564254, 1.19561720770636, 1.1544834399644,
              1.38150029634068, 1.16161919986146, 1.60618779991019, 1.11298877602903)
            invisible(near(cr, expected, "mtcars"))
            nm <- c("Mazda RX4", "Mazda RX4 Wag", "Datsun 710", "Hornet 4 Drive",
              "Hornet Sportabout", "Valiant", "Duster 360", "Merc 240D", "Merc 230",
              "Merc 280", "Merc 280C", "Merc 450SE", "Merc 450SL", "Merc 450SLC",
              "Cadillac Fleetwood", "Lincoln Continental", "Chrysler Imperial",
              "Fiat 128", "Honda Civic", "Toyota Corolla", "Toyota Corona",
              "Dodge Challenger", "AMC Javelin", "Camaro Z28", "Pontiac Firebird",
              "Fiat X1-9", "Porsche 914-2", "Lotus Europa", "Ford Pantera L",
              "Ferrari Dino", "Maserati Bora", "Volvo 142E")
            name_note <- if (identical(names(cr), nm)) "namesok" else paste("names", paste(names(cr), collapse="|"))

            # hat == 1 on the last case. GNU: NaN there, finite elsewhere.
            hat1 <- run("hat1", {
              cr1 <- covratio(lm(c(1, 2, 3, 4) ~ c(0, 0, 0, 1)))
              near(cr1, c(0.374999999999999, 6, 0.375000000000002, NaN), "hat1")
              "hat1ok"
            })

            # Deleting the middle point leaves a perfect line. GNU middle is NaN.
            perfect <- run("perfect", {
              crp <- covratio(lm(c(1, 2, 2, 4, 5) ~ c(1, 2, 3, 4, 5)))
              near(crp, c(4.306640625, 2.77150145772595, NaN, 2.77150145772595, 4.306640625), "perfect")
              "perfectok"
            })

            # n = p + 1. GNU covratio(lm(c(1,2) ~ 1)) is +Inf.
            df0 <- run("df0", {
              cr0 <- covratio(lm(c(1, 2) ~ 1))
              near(cr0, c(Inf, Inf), "df0")
              "df0ok"
            })

            # GNU lm() on zero rows errors ("0 (non-NA) cases").
            empty <- run("empty", {
              covratio(lm(numeric() ~ 1))
              "empty-noerr"
            })

            paste("OK", name_note, hat1, perfect, df0, empty, sep=" || ")
            "#,
        )
        .expect("covratio gnu oracle");
    assert!(
        out.contains("OK") && out.contains("namesok"),
        "covratio oracle script failed: {out}"
    );
    assert!(
        out.contains("hat1ok"),
        "hat==1 covratio did not match GNU: {out}"
    );
    assert!(
        out.contains("perfectok"),
        "perfect-deletion covratio did not match GNU: {out}"
    );
    assert!(
        out.contains("df0ok"),
        "n=p+1 covratio did not match GNU +Inf: {out}"
    );
    assert!(
        out.contains("0 (non-NA) cases"),
        "empty lm did not match GNU's 0 non-NA cases error: {out}"
    );
    eprintln!("covratio edges: {out}");
}
