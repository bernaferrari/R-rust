use regex::Regex;
use std::borrow::Cow;

#[derive(Debug, Clone, Default)]
pub struct OutputNormalizer {
    strip_paths: bool,
    strip_timestamps: bool,
    strip_memory_addresses: bool,
    strip_thread_ids: bool,
    normalize_whitespace: bool,
    ignore_version_strings: bool,
}

impl OutputNormalizer {
    pub fn new() -> Self {
        Self {
            strip_paths: true,
            strip_timestamps: true,
            strip_memory_addresses: true,
            strip_thread_ids: true,
            normalize_whitespace: true,
            ignore_version_strings: true,
        }
    }

    pub fn normalize(&self, input: &[u8]) -> Vec<u8> {
        let s = String::from_utf8_lossy(input);
        let mut out = s.into_owned();

        if self.strip_memory_addresses {
            out = self.strip_memory_addresses(&out);
        }

        if self.strip_paths {
            out = self.strip_file_paths(&out);
        }

        if self.strip_timestamps {
            out = self.strip_timestamps(&out);
        }

        if self.strip_thread_ids {
            out = self.strip_thread_ids(&out);
        }

        if self.ignore_version_strings {
            out = self.strip_version_strings(&out);
        }

        if self.normalize_whitespace {
            out = self.normalize_whitespace(&out);
        }

        out.into_bytes()
    }

    fn strip_memory_addresses(&self, s: &str) -> String {
        let re = Regex::new(r"0x[0-9a-fA-F]+").unwrap();
        re.replace_all(s, "<addr>").into_owned()
    }

    fn strip_file_paths(&self, s: &str) -> String {
        let re = Regex::new(r"(/[^ \n]+/)[^ \n]+").unwrap();
        re.replace_all(s, "<path>").into_owned()
    }

    fn strip_timestamps(&self, s: &str) -> String {
        let re = Regex::new(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d+").unwrap();
        let s = re.replace_all(s, "<timestamp>");

        let re_unix = Regex::new(r"\b\d{10,13}\b").unwrap();
        re_unix.replace_all(&s, "<epoch>").into_owned()
    }

    fn strip_thread_ids(&self, s: &str) -> String {
        let re = Regex::new(r"\b(tid|thread|pid)\s*[=:]\s*\d+").unwrap();
        re.replace_all(s, "$1=<id>").into_owned()
    }

    fn strip_version_strings(&self, s: &str) -> String {
        let re = Regex::new(r"R version \d+\.\d+\.\d+").unwrap();
        re.replace_all(s, "R version <version>").into_owned()
    }

    fn normalize_whitespace(&self, s: &str) -> String {
        s.lines()
            .map(|line| line.trim_end())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn bitwise_equal(&self, a: &[u8], b: &[u8]) -> bool {
        let norm_a = self.normalize(a);
        let norm_b = self.normalize(b);
        norm_a == norm_b
    }
}
