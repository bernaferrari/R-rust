use anyhow::Result;
use clap::Parser;
use r_test_harness::{
    compute_hash, discover_test_cases, read_golden, write_golden, TestProfile, TestRunner,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    baseline: bool,

    #[arg(long)]
    update_golden: bool,

    #[arg(long)]
    profile: Option<String>,

    #[arg(long, default_value = "upstream/tests")]
    test_dir: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let profiles = if let Some(p) = args.profile {
        vec![match p.as_str() {
            "desktop-compat" => TestProfile::DesktopCompat,
            "pure-rust" => TestProfile::PureRust,
            "android" => TestProfile::AndroidHeadless,
            _ => panic!("Unknown profile: {}", p),
        }]
    } else {
        TestProfile::all().to_vec()
    };

    let tests = discover_test_cases(std::path::Path::new(&args.test_dir))?;
    println!("Discovered {} test cases", tests.len());

    let runner = TestRunner::new();

    let mut passed = 0;
    let mut failed = 0;
    let mut mismatched = 0;

    for profile in profiles {
        println!("\n=== Running profile: {} ===", profile.name());

        let allowed_tests: Vec<_> = tests
            .iter()
            .filter(|t| profile.is_test_allowed(&t.name))
            .collect();

        for test in allowed_tests {
            print!("Test {}: ", test.name);

            let res = runner.compare(test)?;

            match res.outcome {
                r_test_harness::TestOutcome::Pass => {
                    println!("PASS");
                    passed += 1;

                    if args.update_golden {
                        let golden_path = std::path::Path::new("profiles/golden")
                            .join(profile.name())
                            .join(&test.name)
                            .with_extension("golden");

                        let stdout_hash = compute_hash(&res.stock_result.stdout);
                        write_golden(&golden_path, &stdout_hash)?;
                    }
                }
                r_test_harness::TestOutcome::Fail => {
                    println!("FAIL");
                    failed += 1;
                }
                r_test_harness::TestOutcome::Mismatch => {
                    println!("MISMATCH");
                    if let Some(diff) = res.diff {
                        println!("{}", diff);
                    }
                    mismatched += 1;
                }
                r_test_harness::TestOutcome::Skipped => {
                    println!("SKIP");
                }
            }
        }
    }

    println!("\n=== Summary ===");
    println!("Passed: {}", passed);
    println!("Failed: {}", failed);
    println!("Mismatched: {}", mismatched);

    if failed + mismatched > 0 {
        std::process::exit(1);
    }

    Ok(())
}
