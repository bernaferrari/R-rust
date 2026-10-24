#![forbid(unsafe_code)]
//! Compact corrections from regex-syntax 0.8.11 to the pinned GNU oracle.
//! GNU R 4.7 devel r90451 / PCRE2 10.49 (2026-09-28).
//! Exhaustive scalar fixtures verify the resulting sets, not just these deltas.
use super::Kind;

pub(super) fn missing(kind: Kind) -> &'static str {
    match kind {
        Kind::Alnum => {
            r"\x{88f}\x{c5c}\x{cdc}\x{a7ce}-\x{a7cf}\x{a7d2}\x{a7d4}\x{a7f1}\x{10940}-\x{10959}\x{10ec5}-\x{10ec7}\x{11db0}-\x{11ddb}\x{11de0}-\x{11de9}\x{16ea0}-\x{16eb8}\x{16ebb}-\x{16ed3}\x{16ff2}-\x{16ff6}\x{187f8}-\x{187ff}\x{18d09}-\x{18d1e}\x{18d80}-\x{18df2}\x{1e6c0}-\x{1e6de}\x{1e6e0}-\x{1e6e2}\x{1e6e4}-\x{1e6e5}\x{1e6e7}-\x{1e6ed}\x{1e6f0}-\x{1e6f4}\x{1e6fe}-\x{1e6ff}\x{2b73a}-\x{2b73f}\x{2cea2}-\x{2cead}\x{323b0}-\x{33479}"
        }
        Kind::Alpha => {
            r"\x{88f}\x{c5c}\x{cdc}\x{a7ce}-\x{a7cf}\x{a7d2}\x{a7d4}\x{a7f1}\x{10940}-\x{10959}\x{10ec5}-\x{10ec7}\x{11db0}-\x{11ddb}\x{16ea0}-\x{16eb8}\x{16ebb}-\x{16ed3}\x{16ff2}-\x{16ff3}\x{187f8}-\x{187ff}\x{18d09}-\x{18d1e}\x{18d80}-\x{18df2}\x{1e6c0}-\x{1e6de}\x{1e6e0}-\x{1e6e2}\x{1e6e4}-\x{1e6e5}\x{1e6e7}-\x{1e6ed}\x{1e6f0}-\x{1e6f4}\x{1e6fe}-\x{1e6ff}\x{2b73a}-\x{2b73f}\x{2cea2}-\x{2cead}\x{323b0}-\x{33479}"
        }
        Kind::Digit => r"\x{11de0}-\x{11de9}",
        Kind::Graph => {
            r"\x{88f}\x{c5c}\x{cdc}\x{1acf}-\x{1add}\x{1ae0}-\x{1aeb}\x{20c1}\x{2b96}\x{a7ce}-\x{a7cf}\x{a7d2}\x{a7d4}\x{a7f1}\x{fbc3}-\x{fbd2}\x{fd90}-\x{fd91}\x{fdc8}-\x{fdce}\x{10940}-\x{10959}\x{10ec5}-\x{10ec7}\x{10ed0}-\x{10ed8}\x{10efa}-\x{10efb}\x{11b60}-\x{11b67}\x{11db0}-\x{11ddb}\x{11de0}-\x{11de9}\x{16ea0}-\x{16eb8}\x{16ebb}-\x{16ed3}\x{16ff2}-\x{16ff6}\x{187f8}-\x{187ff}\x{18d09}-\x{18d1e}\x{18d80}-\x{18df2}\x{1ccfa}-\x{1ccfc}\x{1ceba}-\x{1ced0}\x{1cee0}-\x{1cef0}\x{1e6c0}-\x{1e6de}\x{1e6e0}-\x{1e6f5}\x{1e6fe}-\x{1e6ff}\x{1f6d8}\x{1f777}-\x{1f77a}\x{1f8d0}-\x{1f8d8}\x{1fa54}-\x{1fa57}\x{1fa8a}\x{1fa8e}\x{1fac8}\x{1facd}\x{1faea}\x{1faef}\x{1fbfa}\x{2b73a}-\x{2b73f}\x{2cea2}-\x{2cead}\x{323b0}-\x{33479}"
        }
        Kind::Lower => r"\x{a7cf}\x{16ebb}-\x{16ed3}",
        Kind::Print => {
            r"\x{88f}\x{c5c}\x{cdc}\x{180e}\x{1acf}-\x{1add}\x{1ae0}-\x{1aeb}\x{20c1}\x{2b96}\x{a7ce}-\x{a7cf}\x{a7d2}\x{a7d4}\x{a7f1}\x{fbc3}-\x{fbd2}\x{fd90}-\x{fd91}\x{fdc8}-\x{fdce}\x{10940}-\x{10959}\x{10ec5}-\x{10ec7}\x{10ed0}-\x{10ed8}\x{10efa}-\x{10efb}\x{11b60}-\x{11b67}\x{11db0}-\x{11ddb}\x{11de0}-\x{11de9}\x{16ea0}-\x{16eb8}\x{16ebb}-\x{16ed3}\x{16ff2}-\x{16ff6}\x{187f8}-\x{187ff}\x{18d09}-\x{18d1e}\x{18d80}-\x{18df2}\x{1ccfa}-\x{1ccfc}\x{1ceba}-\x{1ced0}\x{1cee0}-\x{1cef0}\x{1e6c0}-\x{1e6de}\x{1e6e0}-\x{1e6f5}\x{1e6fe}-\x{1e6ff}\x{1f6d8}\x{1f777}-\x{1f77a}\x{1f8d0}-\x{1f8d8}\x{1fa54}-\x{1fa57}\x{1fa8a}\x{1fa8e}\x{1fac8}\x{1facd}\x{1faea}\x{1faef}\x{1fbfa}\x{2b73a}-\x{2b73f}\x{2cea2}-\x{2cead}\x{323b0}-\x{33479}"
        }
        Kind::Punct => r"\x{10ed0}",
        Kind::Upper => r"\x{a7ce}\x{a7d2}\x{a7d4}\x{16ea0}-\x{16eb8}",
        Kind::Word => {
            r"\x{88f}\x{c5c}\x{cdc}\x{1acf}-\x{1add}\x{1ae0}-\x{1aeb}\x{a7ce}-\x{a7cf}\x{a7d2}\x{a7d4}\x{a7f1}\x{10940}-\x{10959}\x{10ec5}-\x{10ec7}\x{10efa}-\x{10efb}\x{11b60}\x{11b62}-\x{11b64}\x{11b66}\x{11db0}-\x{11ddb}\x{11de0}-\x{11de9}\x{16ea0}-\x{16eb8}\x{16ebb}-\x{16ed3}\x{16ff2}-\x{16ff6}\x{187f8}-\x{187ff}\x{18d09}-\x{18d1e}\x{18d80}-\x{18df2}\x{1e6c0}-\x{1e6de}\x{1e6e0}-\x{1e6f5}\x{1e6fe}-\x{1e6ff}\x{2b73a}-\x{2b73f}\x{2cea2}-\x{2cead}\x{323b0}-\x{33479}"
        }
        _ => "",
    }
}

pub(super) fn extra(kind: Kind) -> &'static str {
    match kind {
        Kind::Lower => r"\x{295}",
        Kind::Punct => r"\x{a2}-\x{a6}\x{a8}-\x{a9}\x{ac}\x{ae}-\x{b1}\x{b4}\x{b8}\x{d7}\x{f7}",
        _ => "",
    }
}
