use r_embed::RSession;

/// GNU SClasses.R default `initialize` (653-691): the superclass-argument
/// coerce branches. Expected strings are the pinned oracle's byte-exact
/// output for R Under development (unstable) 2026-08-27 r90451.
fn show(session: &mut RSession, code: &str) -> String {
    match session.eval(code) {
        Ok(text) => text.trim_end().to_string(),
        Err(err) => format!("ERR: {err}"),
    }
}

/// Branch `extends(Classi, Class)`: `.Object <- as(obj, Class, strict=FALSE)`
/// keeps the subclass class, so `new()` rejects the result unless an explicit
/// `setAs` supplies a real downcast.
#[test]
fn initialize_downcast_branch() {
    let mut session = RSession::new().unwrap();
    let setup = r#"local({
        setClass("DcA", slots = c(x = "numeric"))
        setClass("DcB", contains = "DcA", slots = c(y = "numeric"))
        b <- new("DcB", x = 1.5, y = 2.5)
        cat(class(as(b, "DcA", strict = FALSE)), class(as(b, "DcA")), "\n")
        cat(tryCatch(new("DcA", b), error = function(e) conditionMessage(e)), "\n")
        setAs("DcB", "DcA", def = function(from) new("DcA", x = from@x))
        a <- new("DcA", b)
        cat(a@x, class(a), "\n")
    })"#;
    let out = show(&mut session, setup);
    assert_eq!(
        out,
        "DcB DcA \n'initialize' method returned an object of class \"DcB\" instead of the required class \"DcA\" \n1.5 DcA",
        "downcast branch diverged:\n{out}"
    );
}

/// Two classes extending each other: the `extends(Classi, Class)` branch
/// still fires first; the auto-coerce trips on the missing target slot,
/// while the identical-class case copies the object through unchanged.
#[test]
fn initialize_mutual_extends_branch() {
    let mut session = RSession::new().unwrap();
    let out = show(
        &mut session,
        r#"local({
            setClass("MuX", slots = c(a = "numeric"))
            setClass("MuY", slots = c(b = "numeric"))
            suppressWarnings({
                setIs("MuX", "MuY", test = function(from) TRUE)
                setIs("MuY", "MuX", test = function(from) TRUE)
            })
            x <- new("MuX", a = 1)
            cat(isTRUE(extends("MuX", "MuY")), isTRUE(extends("MuY", "MuX")), "\n")
            cat(tryCatch(new("MuY", x), error = function(e) conditionMessage(e)), "\n")
            cat(deparse(new("MuX", x)), "\n")
        })"#,
    );
    assert_eq!(
        out,
        "TRUE TRUE \nno slot of name \"b\" for this object of class \"MuX\" \nnew(\"MuX\", a = 1)",
        "mutual-extends branch diverged:\n{out}"
    );
}

/// Branch `extends(Classi, dataPart)`: `.Object@.Data <- obj` routes through
/// GNU `setDataPart` (strict `as(value, dataClass)`), which also builds and
/// caches the implicit coerce method for the data-part pair.
#[test]
fn initialize_data_part_branch() {
    let mut session = RSession::new().unwrap();
    let out = show(
        &mut session,
        r#"local({
            setClass("DpNum", contains = "numeric", slots = c(tag = "character"))
            n1 <- new("DpNum", 1:3)
            n2 <- new("DpNum", c(10, 20), tag = "t")
            cat(paste(n1@.Data, collapse = ","), typeof(n1@.Data), "\n")
            cat(paste(n2@.Data, collapse = ","), n2@tag, "\n")
            cat(tryCatch(new("DpNum", as.Date("2020-01-01")),
                         error = function(e) conditionMessage(e)), "\n")
        })"#,
    );
    assert_eq!(
        out,
        "1,2,3 integer \n10,20 t \ncannot use object of class \"Date\" in new():  class \"DpNum\" does not extend that class",
        "data-part branch diverged:\n{out}"
    );
}

/// After `new(Class, <integer>)` populated the data part, GNU's cached
/// implicit (integer,numeric) coerce makes a later `new(Class, <factor>)`
/// keep the factor's integer codes in `.Data` (strict=FALSE passthrough).
#[test]
fn initialize_data_part_warm_factor_keeps_integer() {
    let mut session = RSession::new().unwrap();
    let out = show(
        &mut session,
        r#"local({
            setClass("DpNum", contains = "numeric", slots = c(tag = "character"))
            new("DpNum", 1:3)
            v <- new("DpNum", factor(c("a", "b")))
            cat(paste(deparse(v@.Data), collapse = ""), typeof(v@.Data), "\n")
        })"#,
    );
    assert_eq!(out, "1:2 integer", "warm factor data part diverged:\n{out}");
}

/// Matrix data parts from GNU methods/tests/mixinInitialize.R: `new("M", 1:12, ...)`
/// mixes the common-extension coerce with named slot overrides.
#[test]
fn initialize_matrix_data_part_mixin() {
    let mut session = RSession::new().unwrap();
    let out = show(
        &mut session,
        r#"local({
            setClass("DpM", contains = "matrix", representation(fuzz = "numeric"))
            set.seed(113); f1 <- runif(3)
            m <- new("DpM", 1:12, nrow = 3, fuzz = f1)
            cat(paste(dim(m), collapse = "x"),
                identical(as(m, "matrix"), matrix(1:12, nrow = 3)),
                identical(m@fuzz, f1), "\n")
            setClass("DpMsub", contains = "matrix", slots = c(g = "numeric"))
            ms <- new("DpMsub", g = 7)
            m2 <- new("DpM", ms, fuzz = f1)
            cat(paste(dim(m2), collapse = "x"), "\n")
        })"#,
    );
    assert_eq!(
        out, "3x4 TRUE TRUE \n1x1",
        "matrix data part mixin diverged:\n{out}"
    );
}

/// Common-extension branch: `as(.Object, Classi) <- obj` for sibling classes
/// sharing a superclass; slot overrides and the no-common-extension error.
#[test]
fn initialize_common_extension_branch() {
    let mut session = RSession::new().unwrap();
    let out = show(
        &mut session,
        r#"local({
            setClass("CeRoot", slots = c(r = "numeric"))
            setClass("CeB", contains = "CeRoot", slots = c(b = "numeric"))
            setClass("CeC", contains = "CeRoot", slots = c(cc = "numeric"))
            cobj <- new("CeC", r = 1.25, cc = 2.5)
            bobj <- new("CeB", cobj)
            cat(bobj@r, class(bobj), length(bobj@b), "\n")
            cat(new("CeB", cobj, r = 42)@r, "\n")
            setClass("CeMid", contains = "CeRoot", slots = c(m1 = "numeric"))
            setClass("CeD", contains = "CeMid", slots = c(dd = "numeric"))
            setClass("CeE", contains = "CeRoot", slots = c(ee = "numeric"))
            cat(new("CeD", new("CeE", r = 3.5, ee = 1))@r, "\n")
            cat(tryCatch(new("CeB", "someString"),
                         error = function(e) conditionMessage(e)), "\n")
        })"#,
    );
    assert_eq!(
        out,
        "1.25 CeB 0 \n42 \n3.5 \ncannot use object of class \"character\" in new():  class \"CeB\" does not extend that class",
        "common-extension branch diverged:\n{out}"
    );
}

/// Two-super mixing from GNU methods/tests/mixinInitialize.R: unnamed
/// superclass objects fill their own portions, named slots override.
#[test]
fn initialize_mixin_two_supers() {
    let mut session = RSession::new().unwrap();
    let out = show(
        &mut session,
        r#"local({
            setClass("MxA", representation(a = "numeric"))
            a1 <- new("MxA", a = 1.5)
            m1 <- as.matrix(1)
            setClass("MxBB", contains = c("matrix", "MxA"))
            mix <- new("MxBB", m1, a1)
            cat(mix@a, identical(as(mix, "matrix"), m1), "\n")
            mix2 <- new("MxBB", matrix(m1, nrow = 2), a1, a = pi)
            cat(mix2@a, "\n")
        })"#,
    );
    assert_eq!(
        out, "1.5 TRUE \n3.141593",
        "mixin two-supers diverged:\n{out}"
    );
}
