//! Date/DateTime builtin entries.

use super::EvaluatedBuiltin;

pub(super) const BASE_DATETIME: &[EvaluatedBuiltin] = &[
    EvaluatedBuiltin {
        name: "xtfrm.Date",
        handler: crate::mainutils::essentials::do_xtfrm_Date,
    },
    EvaluatedBuiltin {
        name: "xtfrm.POSIXct",
        handler: crate::mainutils::essentials::do_xtfrm_Date,
    },
    EvaluatedBuiltin {
        name: "xtfrm.POSIXt",
        handler: crate::mainutils::essentials::do_xtfrm_Date,
    },
    EvaluatedBuiltin {
        name: "xtfrm.POSIXlt",
        handler: crate::mainutils::essentials::do_xtfrm_Date,
    },
    EvaluatedBuiltin {
        name: "xtfrm.difftime",
        handler: crate::mainutils::essentials::do_xtfrm_Date,
    },
    EvaluatedBuiltin {
        name: "is.numeric.Date",
        handler: crate::mainutils::essentials::do_is_numeric_Date,
    },
    EvaluatedBuiltin {
        name: "is.numeric.POSIXt",
        handler: crate::mainutils::essentials::do_is_numeric_Date,
    },
    EvaluatedBuiltin {
        name: "c.Date",
        handler: crate::mainutils::essentials::do_c_Date,
    },
    EvaluatedBuiltin {
        name: "c.POSIXct",
        handler: crate::mainutils::essentials::do_c_POSIXct,
    },
    EvaluatedBuiltin {
        name: "c.POSIXlt",
        handler: crate::mainutils::essentials::do_c_POSIXlt,
    },
    EvaluatedBuiltin {
        name: "seq.Date",
        handler: crate::mainutils::seq::do_seq,
    },
    EvaluatedBuiltin {
        name: "is.na.POSIXlt",
        handler: crate::mainutils::essentials::do_is_na_POSIXlt,
    },
    EvaluatedBuiltin {
        name: "as.Date",
        handler: crate::mainutils::essentials::do_as_Date,
    },
    EvaluatedBuiltin {
        name: ".Date",
        handler: crate::mainutils::essentials::do_dot_Date,
    },
    EvaluatedBuiltin {
        name: ".POSIXct",
        handler: crate::mainutils::essentials::do_dot_POSIXct,
    },
    EvaluatedBuiltin {
        name: "as.POSIXct",
        handler: crate::mainutils::essentials::do_as_POSIXct,
    },
    EvaluatedBuiltin {
        name: "as.list.Date",
        handler: crate::mainutils::essentials::do_as_list,
    },
    EvaluatedBuiltin {
        name: "julian",
        handler: crate::mainutils::essentials::do_julian,
    },
    EvaluatedBuiltin {
        name: "julian.POSIXt",
        handler: crate::mainutils::essentials::do_julian,
    },
    EvaluatedBuiltin {
        name: "julian.Date",
        handler: crate::mainutils::essentials::do_julian,
    },
    EvaluatedBuiltin {
        name: "as.difftime",
        handler: crate::mainutils::essentials::do_as_difftime,
    },
    EvaluatedBuiltin {
        name: "difftime",
        handler: crate::mainutils::essentials::do_difftime,
    },
    EvaluatedBuiltin {
        name: "ISOdate",
        handler: crate::mainutils::essentials::do_ISOdate,
    },
    EvaluatedBuiltin {
        name: "ISOdatetime",
        handler: crate::mainutils::essentials::do_ISOdatetime,
    },
    EvaluatedBuiltin {
        name: "trunc.Date",
        handler: crate::mainutils::essentials::do_trunc_Date,
    },
    EvaluatedBuiltin {
        name: "format.Date",
        handler: crate::mainutils::essentials::do_format_Date,
    },
    EvaluatedBuiltin {
        name: "as.character.Date",
        handler: crate::mainutils::essentials::do_as_character_Date,
    },
    EvaluatedBuiltin {
        name: "round.Date",
        handler: crate::mainutils::essentials::do_round_Date,
    },
    EvaluatedBuiltin {
        name: "strftime",
        handler: crate::mainutils::essentials::do_strftime,
    },
    EvaluatedBuiltin {
        name: "balancePOSIXlt",
        handler: crate::mainutils::datetime::do_balancePOSIXlt,
    },
    EvaluatedBuiltin {
        name: "unCfillPOSIXlt",
        handler: crate::mainutils::datetime::do_balancePOSIXlt,
    },
    EvaluatedBuiltin {
        name: "update",
        handler: crate::mainutils::essentials::do_update,
    },
    EvaluatedBuiltin {
        name: "update.default",
        handler: crate::mainutils::essentials::do_update,
    },
    EvaluatedBuiltin {
        name: "update.formula",
        handler: crate::library::stats::filter::do_update_formula,
    },
    EvaluatedBuiltin {
        name: "strptime",
        handler: crate::mainutils::datetime::do_strptime,
    },
    EvaluatedBuiltin {
        name: "as.POSIXlt",
        handler: crate::mainutils::datetime::do_as_POSIXlt,
    },
    EvaluatedBuiltin {
        name: "Date2POSIXlt",
        handler: crate::mainutils::datetime::do_D2POSIXlt,
    },
    EvaluatedBuiltin {
        name: "POSIXlt2Date",
        handler: crate::mainutils::datetime::do_POSIXlt2D,
    },
    EvaluatedBuiltin {
        name: "as.Date.POSIXct",
        handler: crate::mainutils::essentials::do_as_Date,
    },
    EvaluatedBuiltin {
        name: "as.Date.POSIXlt",
        handler: crate::mainutils::essentials::do_as_Date,
    },
    EvaluatedBuiltin {
        name: "format.POSIXlt",
        handler: crate::mainutils::datetime::do_format_POSIXlt,
    },
    EvaluatedBuiltin {
        name: "format.POSIXct",
        handler: crate::mainutils::datetime::do_format_POSIXct,
    },
    EvaluatedBuiltin {
        name: "weekdays.POSIXt",
        handler: crate::mainutils::datetime::do_weekdays_POSIXt,
    },
    EvaluatedBuiltin {
        name: "months.POSIXt",
        handler: crate::mainutils::datetime::do_months_POSIXt,
    },
    EvaluatedBuiltin {
        name: "quarters.POSIXt",
        handler: crate::mainutils::datetime::do_quarters_POSIXt,
    },
    EvaluatedBuiltin {
        name: "as.character.POSIXt",
        handler: crate::mainutils::datetime::do_as_character_POSIXt,
    },
    EvaluatedBuiltin {
        name: "as.character.POSIXlt",
        handler: crate::mainutils::datetime::do_as_character_POSIXt,
    },
    EvaluatedBuiltin {
        name: "as.character.POSIXct",
        handler: crate::mainutils::datetime::do_as_character_POSIXt,
    },
    EvaluatedBuiltin {
        name: "as.double.POSIXlt",
        handler: crate::mainutils::datetime::do_as_double_POSIXt,
    },
    EvaluatedBuiltin {
        name: "as.double.POSIXct",
        handler: crate::mainutils::datetime::do_as_double_POSIXt,
    },
    EvaluatedBuiltin {
        name: "as.double.POSIXt",
        handler: crate::mainutils::datetime::do_as_double_POSIXt,
    },
    EvaluatedBuiltin {
        name: "as.numeric.POSIXlt",
        handler: crate::mainutils::datetime::do_as_double_POSIXt,
    },
    EvaluatedBuiltin {
        name: "as.numeric.POSIXct",
        handler: crate::mainutils::datetime::do_as_double_POSIXt,
    },
    EvaluatedBuiltin {
        name: "as.numeric.POSIXt",
        handler: crate::mainutils::datetime::do_as_double_POSIXt,
    },
    EvaluatedBuiltin {
        name: "mean.Date",
        handler: crate::mainutils::essentials::do_mean_Date,
    },
    EvaluatedBuiltin {
        name: "mean.POSIXct",
        handler: crate::mainutils::essentials::do_mean_POSIXct,
    },
    EvaluatedBuiltin {
        name: "mean.POSIXlt",
        handler: crate::mainutils::essentials::do_mean_POSIXlt,
    },
    EvaluatedBuiltin {
        name: "diff.POSIXt",
        handler: crate::mainutils::essentials::do_diff_POSIXt,
    },
    EvaluatedBuiltin {
        name: "diff.Date",
        handler: crate::mainutils::essentials::do_diff_Date,
    },
    EvaluatedBuiltin {
        name: "trunc.POSIXt",
        handler: crate::mainutils::essentials::do_trunc_POSIXt,
    },
    EvaluatedBuiltin {
        name: "round.POSIXt",
        handler: crate::mainutils::essentials::do_round_POSIXt,
    },
    EvaluatedBuiltin {
        name: "seq.POSIXt",
        handler: crate::mainutils::seq::do_seq,
    },
    EvaluatedBuiltin {
        name: "is.finite.POSIXlt",
        handler: crate::mainutils::essentials::do_is_finite_POSIXlt,
    },
    EvaluatedBuiltin {
        name: "is.infinite.POSIXlt",
        handler: crate::mainutils::essentials::do_is_infinite_POSIXlt,
    },
    EvaluatedBuiltin {
        name: "is.nan.POSIXlt",
        handler: crate::mainutils::essentials::do_is_nan_POSIXlt,
    },
    EvaluatedBuiltin {
        name: "weekdays",
        handler: crate::mainutils::essentials::do_weekdays,
    },
    EvaluatedBuiltin {
        name: "months",
        handler: crate::mainutils::essentials::do_months,
    },
    EvaluatedBuiltin {
        name: "quarters",
        handler: crate::mainutils::essentials::do_quarters,
    },
    EvaluatedBuiltin {
        name: "weekdays.Date",
        handler: crate::mainutils::essentials::do_weekdays,
    },
    EvaluatedBuiltin {
        name: "months.Date",
        handler: crate::mainutils::essentials::do_months,
    },
    EvaluatedBuiltin {
        name: "quarters.Date",
        handler: crate::mainutils::essentials::do_quarters,
    },
    EvaluatedBuiltin {
        name: "cut.Date",
        handler: crate::mainutils::essentials::do_cut_Date,
    },
    EvaluatedBuiltin {
        name: "cut.POSIXt",
        handler: crate::mainutils::essentials::do_cut_POSIXt,
    },
    EvaluatedBuiltin {
        name: "print.Date",
        handler: crate::mainutils::essentials::do_print_Date,
    },
    EvaluatedBuiltin {
        name: "date",
        handler: crate::mainutils::platform::do_date,
    },
    EvaluatedBuiltin {
        name: "Sys.time",
        handler: crate::mainutils::essentials::do_Sys_time,
    },
    EvaluatedBuiltin {
        name: "Sys.Date",
        handler: crate::mainutils::essentials::do_Sys_Date,
    },
    EvaluatedBuiltin {
        name: "Sys.timezone",
        handler: crate::mainutils::essentials::do_Sys_timezone,
    },
];
