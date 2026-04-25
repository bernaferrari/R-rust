use r_embed::{RArenaStats, RSession};
use std::env;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
struct Options {
    iterations: usize,
    output_dir: PathBuf,
    check: bool,
}

#[derive(Debug, Clone)]
struct Measurement {
    name: &'static str,
    category: &'static str,
    iterations: usize,
    total: Duration,
    avg: Duration,
    max_avg: Duration,
    arena: RArenaStats,
    bytes: u64,
}

impl Measurement {
    fn avg_ms(&self) -> f64 {
        self.avg.as_secs_f64() * 1000.0
    }

    fn total_ms(&self) -> f64 {
        self.total.as_secs_f64() * 1000.0
    }

    fn max_avg_ms(&self) -> f64 {
        self.max_avg.as_secs_f64() * 1000.0
    }
}

fn main() -> Result<(), String> {
    let options = parse_options()?;
    fs::create_dir_all(&options.output_dir).map_err(|err| err.to_string())?;

    let package_root = create_demo_package(&options.output_dir)?;
    let measurements = vec![
        measure_startup(options.iterations, 250.0)?,
        measure_eval(
            "eval_scalar_loop",
            "eval",
            options.iterations,
            25.0,
            "sum(1:100) + prod(1:6)",
        )?,
        measure_eval(
            "eval_vector_summary",
            "eval",
            options.iterations,
            50.0,
            "x <- 1:1000; c(sum(x), mean(x), length(unique(c(x, x))))",
        )?,
        measure_package_load(options.iterations.clamp(1, 25), 250.0, &package_root)?,
        measure_plot(options.iterations.clamp(1, 25), 600.0)?,
        measure_parallel_sessions(options.iterations.clamp(1, 20), 500.0)?,
    ];

    let markdown = render_markdown(&measurements);
    let json = render_json(&measurements);
    fs::write(options.output_dir.join("performance-summary.md"), &markdown)
        .map_err(|err| err.to_string())?;
    fs::write(options.output_dir.join("performance-summary.json"), &json)
        .map_err(|err| err.to_string())?;

    print!("{markdown}");

    if options.check {
        let failed: Vec<_> = measurements
            .iter()
            .filter(|measurement| measurement.avg > measurement.max_avg)
            .collect();
        if !failed.is_empty() {
            for measurement in failed {
                eprintln!(
                    "{} average {:.3} ms exceeded threshold {:.3} ms",
                    measurement.name,
                    measurement.avg_ms(),
                    measurement.max_avg_ms()
                );
            }
            return Err("performance thresholds failed".to_string());
        }
    }

    Ok(())
}

fn parse_options() -> Result<Options, String> {
    let mut iterations = 100;
    let mut output_dir = PathBuf::from("target/performance");
    let mut check = false;
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--iterations" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--iterations needs a value".to_string())?;
                iterations = value
                    .parse::<usize>()
                    .map_err(|_| "--iterations must be a positive integer".to_string())?;
                if iterations == 0 {
                    return Err("--iterations must be greater than zero".to_string());
                }
            }
            "--output-dir" => {
                output_dir = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--output-dir needs a value".to_string())?,
                );
            }
            "--check" => check = true,
            "--help" | "-h" => {
                println!(
                    "Usage: cargo run -p r-embed --example performance_probe --release -- [--iterations N] [--output-dir DIR] [--check]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Options {
        iterations,
        output_dir,
        check,
    })
}

fn measure_startup(iterations: usize, max_avg_ms: f64) -> Result<Measurement, String> {
    measure("startup_session", "startup", iterations, max_avg_ms, || {
        let session = RSession::new().map_err(|err| err.to_string())?;
        black_box(session.runtime_info().is_active);
        Ok((RArenaStats::default(), 0))
    })
}

fn measure_eval(
    name: &'static str,
    category: &'static str,
    iterations: usize,
    max_avg_ms: f64,
    code: &'static str,
) -> Result<Measurement, String> {
    let mut session = RSession::new().map_err(|err| err.to_string())?;
    measure(name, category, iterations, max_avg_ms, || {
        let output = session.eval_result(code).map_err(|err| err.to_string())?;
        black_box(output.value);
        let stats = session.arena_stats();
        Ok((stats, output.output.len() as u64))
    })
}

fn measure_package_load(
    iterations: usize,
    max_avg_ms: f64,
    package_root: &Path,
) -> Result<Measurement, String> {
    let files_dir = package_root.join("files");
    let cache_dir = package_root.join("cache");
    let library_dir = package_root.join("library");
    measure(
        "package_load_fresh_session",
        "package",
        iterations,
        max_avg_ms,
        || {
            let mut session = RSession::new().map_err(|err| err.to_string())?;
            session
                .configure_android_paths(
                    path_str(&files_dir)?,
                    path_str(&cache_dir)?,
                    Some(path_str(&library_dir)?),
                )
                .map_err(|err| err.to_string())?;
            session
                .load_package("perfdemo")
                .map_err(|err| err.to_string())?;
            let output = session
                .eval_result("perf_value()")
                .map_err(|err| err.to_string())?;
            black_box(output.value);
            Ok((session.arena_stats(), output.output.len() as u64))
        },
    )
}

fn measure_plot(iterations: usize, max_avg_ms: f64) -> Result<Measurement, String> {
    let mut session = RSession::new().map_err(|err| err.to_string())?;
    measure("plot_render_png", "plot", iterations, max_avg_ms, || {
        let bytes = session
            .render_with_dimensions(
                "plot(c(1, 2, 3, 4, 5, 6, 7, 8), c(1, 4, 9, 16, 25, 36, 49, 64), type=\"l\", col=\"steelblue\", main=\"perf\", xlab=\"x\", ylab=\"y\")",
                640,
                360,
            )
            .map_err(|err| err.to_string())?;
        let byte_len = bytes.len() as u64;
        black_box(bytes);
        Ok((session.arena_stats(), byte_len))
    })
}

fn measure_parallel_sessions(iterations: usize, max_avg_ms: f64) -> Result<Measurement, String> {
    measure(
        "parallel_four_sessions",
        "parallel",
        iterations,
        max_avg_ms,
        || {
            let mut handles = Vec::new();
            for worker in 0..4 {
                handles.push(thread::spawn(move || -> Result<RArenaStats, String> {
                    let mut session = RSession::new().map_err(|err| err.to_string())?;
                    let code = format!("sum(1:50) + {worker}");
                    let output = session.eval_result(&code).map_err(|err| err.to_string())?;
                    black_box(output.value);
                    Ok(session.arena_stats())
                }));
            }

            let mut aggregate = RArenaStats::default();
            for handle in handles {
                let stats = handle
                    .join()
                    .map_err(|_| "parallel session worker panicked".to_string())??;
                aggregate.active_nodes += stats.active_nodes;
                aggregate.free_nodes += stats.free_nodes;
                aggregate.retained_bytes += stats.retained_bytes;
            }
            Ok((aggregate, 0))
        },
    )
}

fn measure(
    name: &'static str,
    category: &'static str,
    iterations: usize,
    max_avg_ms: f64,
    mut workload: impl FnMut() -> Result<(RArenaStats, u64), String>,
) -> Result<Measurement, String> {
    eprintln!("measuring {name} ({iterations} iterations)");
    workload()?;
    let start = Instant::now();
    let mut arena = RArenaStats::default();
    let mut bytes = 0;
    for _ in 0..iterations {
        let (next_arena, next_bytes) = workload()?;
        arena = next_arena;
        bytes = next_bytes;
    }
    let total = start.elapsed();
    Ok(Measurement {
        name,
        category,
        iterations,
        total,
        avg: total / iterations as u32,
        max_avg: Duration::from_secs_f64(max_avg_ms / 1000.0),
        arena,
        bytes,
    })
}

fn create_demo_package(output_dir: &Path) -> Result<PathBuf, String> {
    let package_root = output_dir.join("package-corpus");
    let library_dir = package_root.join("library");
    let package_dir = library_dir.join("perfdemo");
    let r_dir = package_dir.join("R");
    fs::create_dir_all(&r_dir).map_err(|err| err.to_string())?;
    fs::create_dir_all(package_root.join("files")).map_err(|err| err.to_string())?;
    fs::create_dir_all(package_root.join("cache")).map_err(|err| err.to_string())?;
    fs::write(
        package_dir.join("DESCRIPTION"),
        "Package: perfdemo\nVersion: 0.1.0\nTitle: Performance Demo\nDescription: Pure R package used by rport performance probes.\nLicense: GPL-2\nNeedsCompilation: no\n",
    )
    .map_err(|err| err.to_string())?;
    fs::write(package_dir.join("NAMESPACE"), "export(perf_value)\n")
        .map_err(|err| err.to_string())?;
    fs::write(r_dir.join("perfdemo.R"), "perf_value <- function() 42L\n")
        .map_err(|err| err.to_string())?;
    Ok(package_root)
}

fn path_str(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("path is not UTF-8: {}", path.display()))
}

fn render_markdown(measurements: &[Measurement]) -> String {
    let mut out = String::new();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    out.push_str("# Performance Summary\n\n");
    out.push_str(&format!("Generated at Unix timestamp `{timestamp}`.\n\n"));
    out.push_str("| Workload | Category | Iterations | Avg ms | Total ms | Threshold ms | Arena nodes | Arena bytes | Output bytes |\n");
    out.push_str("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for measurement in measurements {
        out.push_str(&format!(
            "| `{}` | {} | {} | {:.3} | {:.3} | {:.3} | {} | {} | {} |\n",
            measurement.name,
            measurement.category,
            measurement.iterations,
            measurement.avg_ms(),
            measurement.total_ms(),
            measurement.max_avg_ms(),
            measurement.arena.active_nodes,
            measurement.arena.retained_bytes,
            measurement.bytes
        ));
    }
    out
}

fn render_json(measurements: &[Measurement]) -> String {
    let mut out = String::from("{\n  \"measurements\": [\n");
    for (index, measurement) in measurements.iter().enumerate() {
        if index > 0 {
            out.push_str(",\n");
        }
        out.push_str(&format!(
            "    {{ \"name\": \"{}\", \"category\": \"{}\", \"iterations\": {}, \"avg_ms\": {:.6}, \"total_ms\": {:.6}, \"threshold_avg_ms\": {:.6}, \"arena_active_nodes\": {}, \"arena_free_nodes\": {}, \"arena_retained_bytes\": {}, \"output_bytes\": {} }}",
            measurement.name,
            measurement.category,
            measurement.iterations,
            measurement.avg_ms(),
            measurement.total_ms(),
            measurement.max_avg_ms(),
            measurement.arena.active_nodes,
            measurement.arena.free_nodes,
            measurement.arena.retained_bytes,
            measurement.bytes
        ));
    }
    out.push_str("\n  ]\n}\n");
    out
}
