use r_embed::RSession;

#[test]
fn getanywhere_registered_stats_method_defers_to_namespace_metadata() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r##"
                g0 <- getAnywhere("predict.loess")
                stopifnot(
                    any(g0$where == "registered S3 method for predict from namespace stats"),
                    any(vapply(g0$objs, identical, NA,
                               get("predict.loess", asNamespace("stats"))))
                )

                ## A global override is found in its own environment while
                ## the registered namespace method remains discoverable.
                predict.loess <- function(x) "global override"
                g1 <- getAnywhere("predict.loess")
                stopifnot(any(g1$where == ".GlobalEnv"),
                          any(g1$where == "registered S3 method for predict"),
                          any(g1$where == "namespace:stats"),
                          any(vapply(g1$objs, identical, NA,
                                     get("predict.loess", asNamespace("stats")))))
                rm(predict.loess)

                ## Ordinary base primitive lookup remains visible.
                g2 <- getAnywhere("sum")
                stopifnot(any(g2$where == "package:base"),
                          any(vapply(g2$objs, identical, NA, sum)))
                cat("getanywhere-registered-s3: PASS\n")
                "##
            )
            .unwrap(),
        "getanywhere-registered-s3: PASS\n"
    );
}
