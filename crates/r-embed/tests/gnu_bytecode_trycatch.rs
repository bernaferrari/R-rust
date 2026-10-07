//! Compiled `tryCatch` leaves its arguments unevaluated.
//!
//! GNU R 4.6.1 `compiler::cmpfun` and this port both return `"caught"` for
//! `tryCatch(stop("boom"), error = function(e) "caught")` and for
//! `tryCatch(missing_symbol, error = function(e) "caught")`. The unbound
//! name is an error condition handled by `tryCatch`, not an object-not-found
//! escape that happens before the handler is installed. The embedded RDS
//! blobs are GNU R 4.6.1 version-2 bytecode for those three functions.

use r_embed::RSession;

fn decode_hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}

fn raw_expression(bytes: &[u8]) -> String {
    format!(
        "as.raw(c({}))",
        bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn stream_offset(bytes: &[u8], words: &[i32]) -> usize {
    let encoded = words
        .iter()
        .flat_map(|word| word.to_be_bytes())
        .collect::<Vec<_>>();
    let offsets = bytes
        .windows(encoded.len())
        .enumerate()
        .filter_map(|(offset, candidate)| (candidate == encoded.as_slice()).then_some(offset))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 1, "fixture must contain one tryCatch stream");
    offsets[0]
}

fn field<'a>(report: &'a str, key: &str) -> &'a str {
    report
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("missing {key} in:\n{report}"))
}

const STOP_HEX: &str = "580a00000002000406010002030000000403000000fd000000fe00000015000000030000000d0000000c0000000c00000017000000010000001d000000020000001d000000030000001f000000040000002600000000000000010000000600000006000000fe00000000000000010004000900000008747279436174636800000002000000fe000000f40000000000000006000000fe0000000000000001000400090000000473746f7000000002000000fe0000000000000010000000010004000900000004626f6f6d00000000000000fe000000020000000100040009000000056572726f72000000f40000000100000006000000fe0000000000000001000400090000000866756e6374696f6e00000002000000fe000000020000000100040009000000016500000000000000fb00000000000000fe00000002000000fe000000000000001000000001000400090000000663617567687400000002000000fe00000000000000fe00000000000000fe00000000000000fe00000001000001ff000000150000000d000000080000000c0000001700000001000000220000000200000026000000000000000100000004000000f30000000000000001000002ff0000001000000010000000010004000900000004626f6f6d0000000d0000030d00000008800000000000000000000000000000000000000000000000000000000000000000000402000000010004000900000005636c6173730000001000000001000400090000001065787072657373696f6e73496e646578000000fe000000150000000d000000040000000c00000029000000010000000100000003000000f30000000100000013000000130000000300000402000005ff000000fb000000fe00000015000000010000000d000000040000000c00000010000000000000000100000003000000100000001000000001000400090000000663617567687400000006000000fe00000000000004ff00000002000000fe00000002000005ff00000000000000fb00000000000000fe00000002000000fe000000000000001000000001000400090000000663617567687400000002000000fe00000000000000fe00000000000000fe0000000d0000030d000000048000000000000001000000010000000100000402000006ff0000001000000001000400090000001065787072657373696f6e73496e646578000000fe000000fe0000000d0000030d000000048000000000000000000000000000000000000402000006ff0000001000000001000400090000001065787072657373696f6e73496e646578000000fe00000001000003ff0000000d0000030d0000000c80000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000402000006ff0000001000000001000400090000001065787072657373696f6e73496e646578000000fe";
const MISSING_HEX: &str = "580a00000002000406010002030000000403000000fd000000fe00000015000000030000000d0000000c0000000c00000017000000010000001d000000020000001d000000030000001f0000000400000026000000000000000100000006000000f40000000000000006000000fe00000000000000010004000900000008747279436174636800000002000000fe0000000000000001000400090000000e6d697373696e675f73796d626f6c000000020000000100040009000000056572726f72000000f40000000100000006000000fe0000000000000001000400090000000866756e6374696f6e00000002000000fe000000020000000100040009000000016500000000000000fb00000000000000fe00000002000000fe000000000000001000000001000400090000000663617567687400000002000000fe00000000000000fe00000000000000fe00000000000000fe00000001000001ff000000150000000d000000040000000c0000001400000000000000010000000300000001000002ff000000f3000000000000000d0000030d000000048000000000000001000000010000000100000402000000010004000900000005636c6173730000001000000001000400090000001065787072657373696f6e73496e646578000000fe000000150000000d000000040000000c00000029000000010000000100000003000000f30000000100000013000000130000000300000402000005ff000000fb000000fe00000015000000010000000d000000040000000c00000010000000000000000100000003000000100000001000000001000400090000000663617567687400000006000000fe00000000000004ff00000002000000fe00000002000005ff00000000000000fb00000000000000fe00000002000000fe000000000000001000000001000400090000000663617567687400000002000000fe00000000000000fe00000000000000fe0000000d0000030d000000048000000000000001000000010000000100000402000006ff0000001000000001000400090000001065787072657373696f6e73496e646578000000fe000000fe0000000d0000030d000000048000000000000000000000000000000000000402000006ff0000001000000001000400090000001065787072657373696f6e73496e646578000000fe00000001000003ff0000000d0000030d0000000c80000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000402000006ff0000001000000001000400090000001065787072657373696f6e73496e646578000000fe";
const FORMAL_HEX: &str = "580a00000002000406010002030000000403000000fd000004020000000100040009000000046e616d65000000fb000000fe00000015000000030000000d0000000c0000000c00000017000000010000001d000000020000001d000000030000001f0000000400000026000000000000000100000006000000f40000000000000006000000fe00000000000000010004000900000008747279436174636800000002000000fe00000000000001ff000000020000000100040009000000056572726f72000000f40000000100000006000000fe0000000000000001000400090000000866756e6374696f6e00000002000000fe000000020000000100040009000000016500000000000000fb00000000000000fe00000002000000fe000000000000001000000001000400090000000663617567687400000002000000fe00000000000000fe00000000000000fe00000000000000fe00000001000002ff000000150000000d000000040000000c0000001400000000000000010000000300000001000001ff000000f3000000000000000d0000030d000000048000000000000001000000010000000100000402000000010004000900000005636c6173730000001000000001000400090000001065787072657373696f6e73496e646578000000fe000000150000000d000000040000000c00000029000000010000000100000003000000f30000000100000013000000130000000300000402000005ff000000fb000000fe00000015000000010000000d000000040000000c00000010000000000000000100000003000000100000001000000001000400090000000663617567687400000006000000fe00000000000004ff00000002000000fe00000002000005ff00000000000000fb00000000000000fe00000002000000fe000000000000001000000001000400090000000663617567687400000002000000fe00000000000000fe00000000000000fe0000000d0000030d000000048000000000000001000000010000000100000402000006ff0000001000000001000400090000001065787072657373696f6e73496e646578000000fe000000fe0000000d0000030d000000048000000000000000000000000000000000000402000006ff0000001000000001000400090000001065787072657373696f6e73496e646578000000fe00000001000003ff0000000d0000030d0000000c80000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000402000006ff0000001000000001000400090000001065787072657373696f6e73496e646578000000fe";

#[test]
fn compiled_trycatch_leaves_stop_and_unbound_symbol_unevaluated() {
    let stop = decode_hex(STOP_HEX);
    let missing = decode_hex(MISSING_HEX);
    let formal = decode_hex(FORMAL_HEX);
    // Outer stream: version, GETFUN 1, MAKEPROM 2, MAKEPROM 3, SETTAG 4, CALL 0, RETURN.
    // Point GETFUN at constant 0 (the source call, not a symbol). Retained
    // source would still return "caught"; executing the stream must not.
    let mut mutated = stop.clone();
    let offset = stream_offset(&stop, &[12, 23, 1, 29, 2, 29, 3, 31, 4, 38, 0, 1]);
    assert_eq!(&stop[offset + 8..offset + 12], &1_i32.to_be_bytes());
    mutated[offset + 8..offset + 12].copy_from_slice(&0_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    let script = format!(
        r#"
        flat <- function(x) gsub("[\r\n]+", " ", paste(as.character(x), collapse = " "))
        code_type <- function(fn) {{
            if (!is.function(fn)) return("NA")
            tryCatch(typeof(.Internal(bodyCode(fn))), error = function(e) paste0("ERR:", flat(conditionMessage(e))))
        }}
        call1 <- function(expr) tryCatch(expr, error = function(e) paste0("ERR:", flat(conditionMessage(e))))
        load1 <- function(bytes) {{
            fn <- NULL
            msg <- "ok"
            tryCatch(fn <- unserialize(bytes), error = function(e) msg <<- paste0("ERR:", flat(conditionMessage(e))))
            list(fn = fn, msg = msg)
        }}

        interp_stop <- tryCatch(stop("boom"), error = function(e) "caught")
        interp_miss <- tryCatch(missing_symbol, error = function(e) "caught")
        compile_msg <- "ok"
        f <- NULL
        g <- NULL
        h <- NULL
        kind <- NULL
        meta <- NULL
        tryCatch({{
            f <- compiler::cmpfun(function() tryCatch(stop("boom"), error = function(e) "caught"))
            g <- compiler::cmpfun(function() tryCatch(missing_symbol, error = function(e) "caught"))
            h <- compiler::cmpfun(function(name) tryCatch(name, error = function(e) "caught"))
            kind <- compiler::cmpfun(function(name) {{
                pos <- tryCatch(name, error = function(e) e)
                if (inherits(pos, "error")) "caught-error" else paste0("VALUE:", flat(deparse(pos)))
            }})
            meta <- compiler::cmpfun(function(name) {{
                pos <- tryCatch(name, error = function(e) e)
                paste(deparse(substitute(name)), class(pos)[1], sep = "|")
            }})
        }}, error = function(e) {{
            compile_msg <<- flat(conditionMessage(e))
        }})
        ser <- function(fn) {{
            if (!is.function(fn)) return("NA")
            tryCatch({{ serialize(fn, NULL); "serialized" }}, error = function(e) flat(conditionMessage(e)))
        }}

        gnu_stop <- load1({stop_raw})
        gnu_miss <- load1({miss_raw})
        gnu_form <- load1({form_raw})
        gnu_mut <- load1({mut_raw})

        cat(paste(
            paste0("INTERP_STOP=", interp_stop),
            paste0("INTERP_MISS=", interp_miss),
            paste0("COMPILE=", compile_msg),
            paste0("F_CODE=", code_type(f)),
            paste0("G_CODE=", code_type(g)),
            paste0("H_CODE=", code_type(h)),
            paste0("KIND_CODE=", code_type(kind)),
            paste0("META_CODE=", code_type(meta)),
            paste0("F_SER=", ser(f)),
            paste0("F_VAL=", if (is.function(f)) call1(f()) else "NA"),
            paste0("G_VAL=", if (is.function(g)) call1(g()) else "NA"),
            paste0("H_VAL=", if (is.function(h)) call1(h(missing_symbol)) else "NA"),
            paste0("KIND_VAL=", if (is.function(kind)) call1(kind(missing_symbol)) else "NA"),
            paste0("META_VAL=", if (is.function(meta)) call1(meta(missing_symbol)) else "NA"),
            paste0("GNU_STOP_LOAD=", gnu_stop$msg),
            paste0("GNU_MISS_LOAD=", gnu_miss$msg),
            paste0("GNU_FORM_LOAD=", gnu_form$msg),
            paste0("GNU_MUT_LOAD=", gnu_mut$msg),
            paste0("GNU_STOP_CODE=", code_type(gnu_stop$fn)),
            paste0("GNU_MISS_CODE=", code_type(gnu_miss$fn)),
            paste0("GNU_FORM_CODE=", code_type(gnu_form$fn)),
            paste0("GNU_MUT_CODE=", code_type(gnu_mut$fn)),
            paste0("GNU_STOP_VAL=", if (is.function(gnu_stop$fn)) call1(gnu_stop$fn()) else "NA"),
            paste0("GNU_MISS_VAL=", if (is.function(gnu_miss$fn)) call1(gnu_miss$fn()) else "NA"),
            paste0("GNU_FORM_VAL=", if (is.function(gnu_form$fn)) call1(gnu_form$fn(missing_symbol)) else "NA"),
            paste0("GNU_MUT_VAL=", if (is.function(gnu_mut$fn)) call1(gnu_mut$fn()) else "NA"),
            sep = "\n"
        ))
        "#,
        stop_raw = raw_expression(&stop),
        miss_raw = raw_expression(&missing),
        form_raw = raw_expression(&formal),
        mut_raw = raw_expression(&mutated),
    );
    let report = session
        .eval(&script)
        .unwrap_or_else(|err| panic!("tryCatch probe failed to eval: {err}"));
    let report = report.trim();
    let expect = [
        ("INTERP_STOP", "caught"),
        ("INTERP_MISS", "caught"),
        ("COMPILE", "ok"),
        ("F_CODE", "bytecode"),
        ("G_CODE", "bytecode"),
        ("H_CODE", "bytecode"),
        ("KIND_CODE", "bytecode"),
        ("META_CODE", "bytecode"),
        (
            "F_SER",
            "cannot serialize private bytecode dialect as GNU R BCODESXP",
        ),
        ("F_VAL", "caught"),
        ("G_VAL", "caught"),
        ("H_VAL", "caught"),
        ("KIND_VAL", "caught-error"),
        // Pinned GNU R and the public compiled path preserve the typed
        // objectNotFoundError condition when forcing this promise.
        ("META_VAL", "missing_symbol|objectNotFoundError"),
        ("GNU_STOP_LOAD", "ok"),
        ("GNU_MISS_LOAD", "ok"),
        ("GNU_FORM_LOAD", "ok"),
        ("GNU_MUT_LOAD", "ok"),
        ("GNU_STOP_CODE", "bytecode"),
        ("GNU_MISS_CODE", "bytecode"),
        ("GNU_FORM_CODE", "bytecode"),
        ("GNU_MUT_CODE", "bytecode"),
        ("GNU_STOP_VAL", "caught"),
        ("GNU_MISS_VAL", "caught"),
        ("GNU_FORM_VAL", "caught"),
    ];
    for (key, want) in expect {
        assert_eq!(field(report, key), want, "full report:\n{report}");
    }
    let mutated_value = field(report, "GNU_MUT_VAL");
    assert!(
        mutated_value.starts_with("ERR:"),
        "mutated GETFUN must execute instead of the retained source\nfull report:\n{report}"
    );
    assert_ne!(mutated_value, "caught");
}
