use r_embed::RSession;

const GETANYWHERE_S3_SCRIPT: &str = r##"
    g0 <- getAnywhere("predict.loess")
    stopifnot(
        any(g0$where == "registered S3 method for predict from namespace stats"),
        any(vapply(g0$objs, identical, NA,
                   get("predict.loess", asNamespace("stats"))))
    )

    ## A global override is found in its own environment while the registered
    ## namespace method remains discoverable.
    predict.loess <- function(x) "global override"
    g1 <- getAnywhere("predict.loess")
    stopifnot(any(g1$where == ".GlobalEnv"),
              any(g1$where == "registered S3 method for predict"),
              any(g1$where == "namespace:stats"),
              any(vapply(g1$objs, identical, NA,
                         get("predict.loess", asNamespace("stats")))))
    rm(predict.loess)

    ## Dendrogram methods are owned by stats and dispatch there.
    stopifnot(!exists("as.dendrogram", envir=baseenv(), inherits=FALSE))
    method <- get("as.dendrogram.hclust", asNamespace("stats"))
    stopifnot(identical(environment(method), asNamespace("stats")))
    g2 <- getAnywhere("as.dendrogram.hclust")
    stopifnot(any(g2$where == "registered S3 method for as.dendrogram from namespace stats"),
              any(g2$where == "namespace:stats"))
    tree <- stats::hclust(stats::dist(matrix(c(1, 2, 3, 4), ncol=1)))
    dendrogram <- stats::as.dendrogram(tree)
    stopifnot(identical(class(dendrogram), "dendrogram"))

    as.dendrogram.hclust <- function(object, ...) "global override"
    g3 <- getAnywhere("as.dendrogram.hclust")
    stopifnot(any(g3$where == ".GlobalEnv"),
              any(g3$where == "registered S3 method for as.dendrogram"),
              any(g3$where == "namespace:stats"),
              identical(stats::as.dendrogram(tree), "global override"))
    rm(as.dendrogram.hclust)

    ## Ordinary base primitive metadata remains visible.
    g4 <- getAnywhere("sum")
    stopifnot(any(g4$where == "package:base"),
              any(vapply(g4$objs, identical, NA, sum)))
    cat("getanywhere-registered-s3: PASS\n")
    "##;

fn assert_getanywhere_s3_parity(session: &mut RSession) {
    assert_eq!(
        session
            .eval(GETANYWHERE_S3_SCRIPT)
            .unwrap(),
        "getanywhere-registered-s3: PASS\n"
    );
}

#[test]
fn getanywhere_registered_stats_methods_keep_namespace_ownership() {
    let mut session = RSession::new().unwrap();
    assert_getanywhere_s3_parity(&mut session);
}

#[test]
fn portable_getanywhere_registered_stats_methods_keep_namespace_ownership() {
    let mut session = RSession::new_with_path_policy(r_embed::RuntimePathPolicy::new(
        Vec::new(),
        std::env::temp_dir(),
    ))
    .unwrap();
    assert_getanywhere_s3_parity(&mut session);
}
