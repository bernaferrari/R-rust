use r_embed::RSession;

// Expected lines are GNU R 4.6.1 literals from Rscript --vanilla, not recomputed here.
const PROBE: &str = r####"junk <- options(warn = -1)
fmt <- function(x) {
  if (is.null(x)) return("NULL")
  if (is.na(x)[1] && length(x) == 1 && !is.character(x)) return(paste0(fmt_type(x)))
  if (length(x) == 0) return(paste0(typeof(x), "0"))
  paste(x, collapse = ",")
}
fmt_type <- function(x) {
  if (is.logical(x)) paste0("L:", paste(x, collapse = ",")) else paste(x, collapse = ",")
}
say <- function(...) cat(paste0(...), "\n", sep = "")
safe <- function(expr) tryCatch(expr, error = function(e) paste0("ERR:", conditionMessage(e)))

say("small_stats ", safe(fmt(list(a = 1, stats = 2, c = 3)[["stats"]])))
say("small_names ", safe(fmt(names(list(a = 1, stats = 2)))))
say("small_exactT ", safe(fmt(list(a = 1, stats = 2, c = 3)[["stats", exact = TRUE]])))
say("small_exactNA ", safe(fmt(list(a = 1, stats = 2, c = 3)[["stats", exact = NA]])))
say("small_exactF ", safe(fmt(list(a = 1, stats = 2, c = 3)[["stats", exact = FALSE]])))

for (n in c(0, 10, 100, 1000, 5000, 20000)) {
  v <- c(rep(FALSE, n), TRUE)
  say(
    "any_lgl_", n, " ",
    safe(fmt(any(v))),
    " len ", safe(as.character(length(v))),
    " last ", safe(fmt(v[length(v)]))
  )
}

ns_list <- c(1:80, 96, 100, 127, 128, 129, 150, 200, 256)
lit_builder <- function(n) {
  if (n <= 1) return(list(stats = 99))
  txt <- paste0(
    "list(",
    paste0("p", seq_len(n - 1), "=", seq_len(n - 1), collapse = ","),
    ",stats=99)"
  )
  eval(parse(text = txt))
}
nms_builder <- function(n) {
  x <- as.list(c(if (n > 1) seq_len(n - 1) else integer(), 99))
  names(x) <- c(if (n > 1) paste0("p", seq_len(n - 1)) else character(), "stats")
  x
}
probe_list <- function(label, builder) {
  for (n in ns_list) {
    det <- tryCatch({
      x <- builder(n)
      got <- x[["stats"]]
      if (identical(got, 99)) {
        NULL
      } else {
        nm <- names(x)
        pos <- 0
        if (!is.null(nm)) {
          for (i in seq_along(nm)) if (identical(nm[[i]], "stats")) pos <- i
        }
        by_pos <- if (pos > 0) x[[pos]] else NULL
        paste0(
          "n=", n,
          " got=", fmt(got),
          " len=", length(x),
          " nlen=", length(nm),
          " pos=", pos,
          " name_at=", if (pos > 0) nm[[pos]] else "NA",
          " by_pos=", fmt(by_pos),
          " exactT=", fmt(x[["stats", exact = TRUE]]),
          " exactNA=", fmt(x[["stats", exact = NA]]),
          " exactF=", fmt(x[["stats", exact = FALSE]])
        )
      }
    }, error = function(e) paste0("n=", n, " ERR:", conditionMessage(e)))
    if (!is.null(det)) {
      say(label, " ", det)
      return(invisible())
    }
  }
  say(label, " none")
}
probe_list("lit", lit_builder)
probe_list("nms", nms_builder)

ns_any <- c(0:80, 96, 100, 127, 128, 129, 150, 151, 200, 256, 500, 1000, 5000, 20000)
ns_many <- c(0:80, 96, 100, 127, 128, 129, 150, 151, 200, 256)
probe_any <- function(label, builder, ns) {
  for (n in ns) {
    det <- tryCatch({
      tab <- builder(n)
      hit <- 0
      for (i in seq_along(tab)) if (identical(tab[[i]], "t.test")) hit <- i
      scalar <- if (hit > 0) isTRUE(tab[[hit]] == "t.test") else FALSE
      vec <- "t.test" == tab
      vec_at <- if (hit > 0 && hit <= length(vec)) isTRUE(vec[[hit]]) else FALSE
      anyv <- isTRUE(any("t.test" == tab))
      anyr <- isTRUE(any(tab == "t.test"))
      ok <- anyv && anyr && hit == length(tab) &&
        identical(tab[[length(tab)]], "t.test") && scalar && vec_at &&
        length(vec) == length(tab)
      if (ok) {
        NULL
      } else {
        paste0(
          "n=", n,
          " len=", length(tab),
          " hit=", hit,
          " last=", if (length(tab)) fmt(tab[[length(tab)]]) else "EMPTY",
          " scalar=", scalar,
          " vec_at=", vec_at,
          " veclen=", length(vec),
          " any=", anyv,
          " anyr=", anyr
        )
      }
    }, error = function(e) paste0("n=", n, " ERR:", conditionMessage(e)))
    if (!is.null(det)) {
      say(label, " ", det)
      return(invisible())
    }
  }
  say(label, " none")
}
probe_any("any_rep", function(n) c(rep("no", n), "t.test"), ns_any)
probe_any("any_idx", function(n) {
  v <- rep("no", n + 1)
  v[n + 1] <- "t.test"
  v
}, ns_any)
probe_any("any_many", function(n) {
  if (n == 0) return(c("t.test"))
  txt <- paste0("c(", paste(rep("\"no\"", n), collapse = ","), ",\"t.test\")")
  eval(parse(text = txt))
}, ns_many)
probe_any("any_docall", function(n) {
  args <- list()
  if (n > 0) for (i in seq_len(n)) args[[i]] <- "no"
  args[[n + 1]] <- "t.test"
  do.call(c, args)
}, ns_many)

stopList <- list(
  base = c("all.equal", "all.names", "all.vars", "as.data.frame.vector",
    "format.info", "format.pval", "max.col", "qr.Q", "qr.R", "qr.X",
    "qr.coef", "qr.fitted", "qr.qty", "qr.qy", "qr.resid", "qr.solve",
    "rep.int", "seq.int", "sort.int", "sort.list"),
  AMORE = "sim.MLPnet",
  BSDA = "sign.test",
  BiocGenerics = "rep.int",
  ChemometricsWithR = "lda.loofun",
  ElectoGraph = "plot.wedding.cake",
  FrF2 = "all.2fis.clear.catlg",
  GLDEX = c("hist.su", "pretty.su"),
  Hmisc = c("abs.error.pred", "all.digits", "all.is.numeric", "format.df",
    "format.pval", "t.test.cluster"),
  HyperbolicDist = "log.hist",
  MASS = c("frequency.polygon", "gamma.dispersion", "gamma.shape", "hist.FD",
    "hist.scott"),
  LinearizedSVR = "sigma.est",
  Matrix = c("qr.Q", "qr.R", "qr.coef", "qr.fitted", "qr.qty", "qr.qy",
    "qr.resid"),
  PerformanceAnalytics = c("mean.LCL", "mean.UCL", "mean.geometric",
    "mean.stderr"),
  RCurl = "merge.list",
  RNetCDF = c("close.nc", "dim.def.nc", "dim.inq.nc", "dim.rename.nc",
    "open.nc", "print.nc"),
  Rmpfr = c("mpfr.is.0", "mpfr.is.integer"),
  SMPracticals = "exp.gibbs",
  SparseM = c("as.matrix.csc", "as.matrix.csr", "as.matrix.ssc",
    "as.matrix.ssr", "as.matrix.coo", "is.matrix.csc", "is.matrix.csr",
    "is.matrix.ssc", "is.matrix.ssr", "is.matrix.coo"),
  TANOVA = "sigma.hat",
  TeachingDemos = "sigma.test",
  XML = "text.SAX",
  ape = "sort.index",
  arm = "sigma.hat",
  assist = "chol.new",
  boot = "exp.tilt",
  car = "scatterplot.matrix",
  calibrator = "t.fun",
  clusterfly = "ggobi.som",
  coda = "as.mcmc.list",
  crossdes = "all.combn",
  ctv = "update.views",
  deSolve = "plot.1D",
  effects = "all.effects",
  elliptic = "sigma.laurent",
  equivalence = "sign.boot",
  fields = c("qr.q2ty", "qr.yq2"),
  gbm = c("pretty.gbm.tree", "quantile.rug"),
  genetics = "diseq.ci",
  gpclib = "scale.poly",
  grDevices = "boxplot.stats",
  graphics = c("close.screen", "plot.design", "plot.new", "plot.window",
    "plot.xy", "split.screen"),
  ic.infer = "all.R2",
  hier.part = "all.regs",
  lasso2 = "qr.rtr.inv",
  latticeExtra = "xyplot.list",
  locfit = c("density.lf", "plot.eval"),
  moments = c("all.cumulants", "all.moments"),
  mosaic = "t.test",
  mratios = c("t.test.ration", "t.test.ratio.default", "t.test.ratio.formula"),
  ncdf = c("open.ncdf", "close.ncdf", "dim.create.ncdf", "dim.def.ncdf",
    "dim.inq.ncdf", "dim.same.ncdf"),
  plyr = c("rbind.fill", "rbind.fill.matrix"),
  quadprog = c("solve.QP", "solve.QP.compact"),
  reposTools = "update.packages2",
  reshape = "all.vars.character",
  rgeos = "scale.poly",
  rowr = "cbind.fill",
  sac = "cumsum.test",
  sfsmisc = "cumsum.test",
  sm = "print.graph",
  spatstat = "lengths.psp",
  splusTimeDate = "sort.list",
  splusTimeSeries = "sort.list",
  stats = c("anova.lmlist", "expand.model.frame", "fitted.values",
    "influence.measures", "lag.plot", "qr.influence", "t.test",
    "plot.spec.phase", "plot.spec.coherency"),
  stremo = "sigma.hat",
  supclust = c("sign.change", "sign.flip"),
  tensorA = "chol.tensor",
  utils = c("close.socket", "flush.console", "update.packages"),
  wavelets = "plot.dwt.multiple"
)

say("stop_len ", safe(as.character(length(stopList))))
say("stop_names ", safe(fmt(names(stopList))))
pos <- 0
nm <- names(stopList)
if (!is.null(nm)) for (i in seq_along(nm)) if (identical(nm[[i]], "stats")) pos <- i
say("stop_pos ", pos)
say("stop_bypos ", safe(fmt(if (pos > 0) stopList[[pos]] else NULL)))
say("stop_stats ", safe(fmt(stopList[["stats"]])))
say("stop_stats_T ", safe(fmt(stopList[["stats", exact = TRUE]])))
say("stop_stats_NA ", safe(fmt(stopList[["stats", exact = NA]])))
say("stop_stats_F ", safe(fmt(stopList[["stats", exact = FALSE]])))
say("stop_dollar ", safe(fmt(stopList$stats)))
say("stop_base ", safe(fmt(stopList[["base"]])))
say("stop_wavelets ", safe(fmt(stopList[["wavelets"]])))
say("stop_utils ", safe(fmt(stopList[["utils"]])))
say("stop_mosaic ", safe(fmt(stopList[["mosaic"]])))

alt <- list()
for (i in seq_along(stopList)) alt[[i]] <- if (pos > 0 && i == pos) 99 else i
names(alt) <- nm
say("alt_stats ", safe(fmt(alt[["stats"]])))
say("alt_exactNA ", safe(fmt(alt[["stats", exact = NA]])))

report_tab <- function(label, tab) {
  det <- tryCatch({
    if (is.character(tab) && length(tab) == 1 && startsWith(tab, "ERR:")) {
      return(tab)
    }
    hit <- 0
    for (i in seq_along(tab)) if (identical(tab[[i]], "t.test")) hit <- i
    scalar <- if (hit > 0) isTRUE(tab[[hit]] == "t.test") else FALSE
    vec <- "t.test" == tab
    vec_at <- if (hit > 0 && hit <= length(vec)) isTRUE(vec[[hit]]) else FALSE
    paste0(
      "len=", length(tab),
      " hit=", hit,
      " last=", if (length(tab)) fmt(tab[[length(tab)]]) else "EMPTY",
      " scalar=", scalar,
      " vec_at=", vec_at,
      " veclen=", length(vec),
      " any=", isTRUE(any("t.test" == tab)),
      " anyr=", isTRUE(any(tab == "t.test")),
      " present=", hit > 0
    )
  }, error = function(e) paste0("ERR:", conditionMessage(e)))
  say(label, " ", det)
}

args <- list()
for (i in seq_along(stopList)) args[[i]] <- stopList[[i]]
report_tab("flat_docall", safe(do.call(c, args)))
txt <- paste0("c(", paste0("stopList[[", seq_along(stopList), "]]", collapse = ","), ")")
report_tab("flat_onec", safe(eval(parse(text = txt))))
acc <- character()
for (i in seq_along(stopList)) acc <- c(acc, stopList[[i]])
report_tab("flat_iter", acc)
report_tab("flat_unlist", safe(c(unlist(stopList))))
report_tab("stats_only", safe(stopList[["stats"]]))
say("done")
"####;

const GNU: &str = r####"small_stats 2
small_names a,stats
small_exactT 2
small_exactNA 2
small_exactF 2
any_lgl_0 TRUE len 1 last TRUE
any_lgl_10 TRUE len 11 last TRUE
any_lgl_100 TRUE len 101 last TRUE
any_lgl_1000 TRUE len 1001 last TRUE
any_lgl_5000 TRUE len 5001 last TRUE
any_lgl_20000 TRUE len 20001 last TRUE
lit none
nms none
any_rep none
any_idx none
any_many none
any_docall none
stop_len 69
stop_names base,AMORE,BSDA,BiocGenerics,ChemometricsWithR,ElectoGraph,FrF2,GLDEX,Hmisc,HyperbolicDist,MASS,LinearizedSVR,Matrix,PerformanceAnalytics,RCurl,RNetCDF,Rmpfr,SMPracticals,SparseM,TANOVA,TeachingDemos,XML,ape,arm,assist,boot,car,calibrator,clusterfly,coda,crossdes,ctv,deSolve,effects,elliptic,equivalence,fields,gbm,genetics,gpclib,grDevices,graphics,ic.infer,hier.part,lasso2,latticeExtra,locfit,moments,mosaic,mratios,ncdf,plyr,quadprog,reposTools,reshape,rgeos,rowr,sac,sfsmisc,sm,spatstat,splusTimeDate,splusTimeSeries,stats,stremo,supclust,tensorA,utils,wavelets
stop_pos 64
stop_bypos anova.lmlist,expand.model.frame,fitted.values,influence.measures,lag.plot,qr.influence,t.test,plot.spec.phase,plot.spec.coherency
stop_stats anova.lmlist,expand.model.frame,fitted.values,influence.measures,lag.plot,qr.influence,t.test,plot.spec.phase,plot.spec.coherency
stop_stats_T anova.lmlist,expand.model.frame,fitted.values,influence.measures,lag.plot,qr.influence,t.test,plot.spec.phase,plot.spec.coherency
stop_stats_NA anova.lmlist,expand.model.frame,fitted.values,influence.measures,lag.plot,qr.influence,t.test,plot.spec.phase,plot.spec.coherency
stop_stats_F anova.lmlist,expand.model.frame,fitted.values,influence.measures,lag.plot,qr.influence,t.test,plot.spec.phase,plot.spec.coherency
stop_dollar anova.lmlist,expand.model.frame,fitted.values,influence.measures,lag.plot,qr.influence,t.test,plot.spec.phase,plot.spec.coherency
stop_base all.equal,all.names,all.vars,as.data.frame.vector,format.info,format.pval,max.col,qr.Q,qr.R,qr.X,qr.coef,qr.fitted,qr.qty,qr.qy,qr.resid,qr.solve,rep.int,seq.int,sort.int,sort.list
stop_wavelets plot.dwt.multiple
stop_utils close.socket,flush.console,update.packages
stop_mosaic t.test
alt_stats 99
alt_exactNA 99
flat_docall len=151 hit=141 last=plot.dwt.multiple scalar=TRUE vec_at=TRUE veclen=151 any=TRUE anyr=TRUE present=TRUE
flat_onec len=151 hit=141 last=plot.dwt.multiple scalar=TRUE vec_at=TRUE veclen=151 any=TRUE anyr=TRUE present=TRUE
flat_iter len=151 hit=141 last=plot.dwt.multiple scalar=TRUE vec_at=TRUE veclen=151 any=TRUE anyr=TRUE present=TRUE
flat_unlist len=151 hit=141 last=plot.dwt.multiple scalar=TRUE vec_at=TRUE veclen=151 any=TRUE anyr=TRUE present=TRUE
stats_only len=9 hit=7 last=plot.spec.coherency scalar=TRUE vec_at=TRUE veclen=9 any=TRUE anyr=TRUE present=TRUE
done
"####;

#[test]
fn named_list_subset_and_any_match_gnu() {
    let mut session = RSession::new().unwrap();
    let got = session.eval(PROBE).unwrap();
    assert_eq!(got.trim(), GNU.trim());
}
