//! String and character builtin entries.

use super::EvaluatedBuiltin;

pub(super) const BASE_STRINGS: &[EvaluatedBuiltin] = &[
    EvaluatedBuiltin {
        name: "character",
        handler: crate::mainutils::essentials::do_character_constructor,
    },
    EvaluatedBuiltin {
        name: "paste",
        handler: crate::mainutils::essentials::do_paste,
    },
    EvaluatedBuiltin {
        name: "paste0",
        handler: crate::mainutils::essentials::do_paste0,
    },
    EvaluatedBuiltin {
        name: "as.character",
        handler: crate::mainutils::essentials::do_as_character,
    },
    EvaluatedBuiltin {
        name: "nchar",
        handler: crate::mainutils::essentials::do_nchar,
    },
    EvaluatedBuiltin {
        name: "substr",
        handler: crate::mainutils::essentials::do_substr,
    },
    EvaluatedBuiltin {
        name: "substr<-",
        handler: crate::mainutils::essentials::do_substrgets,
    },
    EvaluatedBuiltin {
        // base::substring: R-level wrapper rep_lens text to the common
        // length before the internal (character.R); do_substring mirrors
        // that, so `substring(s, c(1,2), c(2,3))` returns 2 elements
        // while plain `substr` iterates len(x) only.
        name: "substring",
        handler: crate::mainutils::essentials::do_substring,
    },
    EvaluatedBuiltin {
        name: "strrep",
        handler: crate::mainutils::essentials::do_strrep,
    },
    EvaluatedBuiltin {
        name: "encodeString",
        handler: crate::mainutils::essentials::do_encodeString,
    },
    EvaluatedBuiltin {
        name: "tolower",
        handler: crate::mainutils::essentials::do_tolower,
    },
    EvaluatedBuiltin {
        name: "toupper",
        handler: crate::mainutils::essentials::do_toupper,
    },
    EvaluatedBuiltin {
        name: "casefold",
        handler: crate::mainutils::essentials::do_casefold,
    },
    EvaluatedBuiltin {
        name: "make.names",
        handler: crate::mainutils::essentials::do_make_names,
    },
    EvaluatedBuiltin {
        name: "utf8ToInt",
        handler: crate::mainutils::essentials::do_utf8ToInt,
    },
    EvaluatedBuiltin {
        name: "intToUtf8",
        handler: crate::mainutils::essentials::do_intToUtf8,
    },
    EvaluatedBuiltin {
        name: "validUTF8",
        handler: crate::mainutils::essentials::do_validUTF8,
    },
    EvaluatedBuiltin {
        name: "validEnc",
        handler: crate::mainutils::essentials::do_validEnc,
    },
    EvaluatedBuiltin {
        name: "format.hexmode",
        handler: crate::mainutils::essentials::do_format_hexmode,
    },
    EvaluatedBuiltin {
        name: "as.character.hexmode",
        handler: crate::mainutils::essentials::do_as_character_hexmode,
    },
    EvaluatedBuiltin {
        name: "format.octmode",
        handler: crate::mainutils::essentials::do_format_octmode,
    },
    EvaluatedBuiltin {
        name: "as.character.octmode",
        handler: crate::mainutils::essentials::do_as_character_octmode,
    },
    EvaluatedBuiltin {
        name: "formatC",
        handler: crate::mainutils::essentials::do_formatC,
    },
    EvaluatedBuiltin {
        name: "iconv",
        handler: crate::mainutils::essentials::do_iconv,
    },
    EvaluatedBuiltin {
        name: "iconvlist",
        handler: crate::mainutils::essentials::do_iconvlist,
    },
    EvaluatedBuiltin {
        name: "enc2utf8",
        handler: crate::mainutils::essentials::do_enc2utf8,
    },
    EvaluatedBuiltin {
        name: "trimws",
        handler: crate::mainutils::essentials::do_trimws,
    },
    EvaluatedBuiltin {
        name: "sprintf",
        handler: crate::mainutils::sprintf_main::do_sprintf,
    },
    EvaluatedBuiltin {
        name: "gsub",
        handler: crate::mainutils::essentials::do_gsub,
    },
    EvaluatedBuiltin {
        name: "sub",
        handler: crate::mainutils::essentials::do_sub,
    },
    EvaluatedBuiltin {
        name: "grep",
        handler: crate::mainutils::essentials::do_grep,
    },
    EvaluatedBuiltin {
        name: "grepl",
        handler: crate::mainutils::essentials::do_grepl,
    },
    EvaluatedBuiltin {
        name: "agrep",
        handler: crate::mainutils::essentials::do_agrep,
    },
    EvaluatedBuiltin {
        name: "agrepl",
        handler: crate::mainutils::essentials::do_agrepl,
    },
    EvaluatedBuiltin {
        name: "strsplit",
        handler: crate::mainutils::essentials::do_strsplit,
    },
    EvaluatedBuiltin {
        name: "chartr",
        handler: crate::mainutils::essentials::do_chartr,
    },
    EvaluatedBuiltin {
        name: "format.info",
        handler: crate::mainutils::essentials::do_format_info,
    },
    EvaluatedBuiltin {
        name: "noquote",
        handler: crate::mainutils::essentials::do_noquote,
    },
    EvaluatedBuiltin {
        name: "format.data.frame",
        handler: crate::mainutils::essentials::do_format_data_frame,
    },
    EvaluatedBuiltin {
        name: "startsWith",
        handler: crate::mainutils::essentials::do_startsWith,
    },
    EvaluatedBuiltin {
        name: "endsWith",
        handler: crate::mainutils::essentials::do_endsWith,
    },
    EvaluatedBuiltin {
        name: "print.character",
        handler: crate::mainutils::essentials::do_print_character,
    },
    EvaluatedBuiltin {
        name: "summary.character",
        handler: crate::mainutils::essentials::do_summary_character,
    },
    EvaluatedBuiltin {
        name: "subset",
        handler: crate::mainutils::essentials::do_subset_named,
    },
    EvaluatedBuiltin {
        name: "type.convert",
        handler: crate::mainutils::essentials::do_type_convert,
    },
    EvaluatedBuiltin {
        name: "rawToChar",
        handler: crate::mainutils::essentials::do_rawToChar,
    },
    EvaluatedBuiltin {
        name: "charToRaw",
        handler: crate::mainutils::essentials::do_charToRaw,
    },
    EvaluatedBuiltin {
        name: "toString",
        handler: crate::mainutils::essentials::do_toString,
    },
];
