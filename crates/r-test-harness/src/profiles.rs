use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TestProfile {
    DesktopCompat,
    PureRust,
    AndroidHeadless,
}

impl TestProfile {
    pub fn all() -> &'static [Self] {
        &[
            TestProfile::DesktopCompat,
            TestProfile::PureRust,
            TestProfile::AndroidHeadless,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            TestProfile::DesktopCompat => "desktop-compat",
            TestProfile::PureRust => "pure-rust",
            TestProfile::AndroidHeadless => "android",
        }
    }

    pub fn cargo_features(&self) -> &'static [&'static str] {
        match self {
            TestProfile::DesktopCompat => &["desktop-compat"],
            TestProfile::PureRust => &["desktop-pure-rust"],
            TestProfile::AndroidHeadless => &["android-headless"],
        }
    }

    pub fn excluded_tests(&self) -> &'static [&'static str] {
        match self {
            TestProfile::DesktopCompat => &[],
            TestProfile::PureRust => &["library/stats/arima", "library/stats/nls"],
            TestProfile::AndroidHeadless => &["graphics", "grdevices", "interactive"],
        }
    }

    pub fn is_test_allowed(&self, test_name: &str) -> bool {
        !self
            .excluded_tests()
            .iter()
            .any(|excl| test_name.starts_with(excl))
    }

    pub fn default_timeout_ms(&self) -> u64 {
        match self {
            TestProfile::DesktopCompat => 30000,
            TestProfile::PureRust => 60000,
            TestProfile::AndroidHeadless => 120000,
        }
    }
}
