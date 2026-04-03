mod comparisons;
mod test_cauchy;
mod test_exponential;
mod test_lnorm;
mod test_logistic;
mod test_nbeta;
mod test_nbinom;
mod test_nchisq;
mod test_nf_dist;
mod test_normal;
mod test_nt_dist;
mod test_signrank;
mod test_special;
mod test_tukey;
mod test_uniform;
mod test_utils;
mod test_weibull;
mod test_wilcox;

fn main() {
    println!("Running rmath numerical equivalence tests...\n");

    let mut passed = 0;
    let mut failed = 0;

    let results: Vec<(&str, Result<(), String>)> = vec![
        ("Utilities", test_utils::run_tests()),
        ("Uniform", test_uniform::run_tests()),
        ("Exponential", test_exponential::run_tests()),
        ("Cauchy", test_cauchy::run_tests()),
        ("Normal", test_normal::run_tests()),
        ("Logistic", test_logistic::run_tests()),
        ("Weibull", test_weibull::run_tests()),
        ("Lognormal", test_lnorm::run_tests()),
        ("Negative Binomial", test_nbinom::run_tests()),
        ("Noncentral Chi-sq", test_nchisq::run_tests()),
        ("Noncentral T", test_nt_dist::run_tests()),
        ("Noncentral F", test_nf_dist::run_tests()),
        ("Noncentral Beta", test_nbeta::run_tests()),
        ("Wilcoxon", test_wilcox::run_tests()),
        ("Signed Rank", test_signrank::run_tests()),
        ("Tukey", test_tukey::run_tests()),
        ("Special Functions", test_special::run_tests()),
    ];

    for (name, result) in &results {
        match result {
            Ok(()) => {
                println!("  [PASS] {}", name);
                passed += 1;
            }
            Err(e) => {
                println!("  [FAIL] {}", name);
                println!("         {}", e);
                failed += 1;
            }
        }
    }

    println!("\nResults: {} passed, {} failed", passed, failed);
    if failed > 0 {
        std::process::exit(1);
    }
}
