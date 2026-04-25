use r_embed::{AndroidRuntimePaths, CancellationToken, RSession, RValue};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let out_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/android-showcase"));

    std::fs::create_dir_all(&out_dir)?;
    let library_dir = out_dir.join("bundled-library");
    write_demo_package(&library_dir.join("androiddemo"))?;

    let mut transcript = String::new();
    transcript.push_str("RPort Android showcase\n");
    transcript.push_str("======================\n\n");

    let session_a_paths = runtime_paths(&out_dir, "session-a", &library_dir)?;
    let session_b_paths = runtime_paths(&out_dir, "session-b", &library_dir)?;

    let mut session_a = RSession::new()?;
    session_a.configure_android_runtime(&session_a_paths)?;
    transcript.push_str("Session A runtime paths:\n");
    transcript.push_str(&format!(
        "  libraries: {:?}\n  temp: {}\n\n",
        session_a.runtime_info().library_paths,
        session_a.runtime_info().temp_dir
    ));

    let packages = session_a.installed_packages();
    transcript.push_str("Installed packages:\n");
    for package in &packages {
        transcript.push_str(&format!("  {} {}\n", package.name, package.version));
    }
    transcript.push('\n');

    session_a.load_package("androiddemo")?;
    let s3 = session_a.eval_result(r#"demo_label(demo_object("Session A"))"#)?;
    transcript.push_str("Pure-R package and S3 dispatch:\n");
    transcript.push_str(&format!("  output: {}\n", s3.output.trim()));
    transcript.push_str(&format!("  value: {}\n\n", describe_value(&s3.value)));

    let typed = session_a.eval_result("demo_value(41)")?;
    transcript.push_str("Typed result:\n");
    transcript.push_str(&format!("  output: {}\n", typed.output.trim()));
    transcript.push_str(&format!("  value: {}\n\n", describe_value(&typed.value)));

    let line_plot = session_a.render_with_dimensions(
        r#"plot(c(1, 2, 3, 4), c(1, 4, 9, 16), type = "l", col = "blue", lwd = 2, main = "Android growth", xlab = "x", ylab = "x^2")"#,
        720,
        480,
    )?;
    let line_plot_path = out_dir.join("line-plot.png");
    std::fs::write(&line_plot_path, line_plot)?;
    transcript.push_str(&format!("Plot artifact: {}\n", line_plot_path.display()));

    let point_plot = session_a.render_with_dimensions(
        r#"plot(c(1, 2, 3, 4), c(3, 1, 4, 2), type = "b", col = "green", cex = 1.3, main = "Android points", xlab = "sample", ylab = "value")"#,
        720,
        480,
    )?;
    let point_plot_path = out_dir.join("point-plot.png");
    std::fs::write(&point_plot_path, point_plot)?;
    transcript.push_str(&format!("Plot artifact: {}\n\n", point_plot_path.display()));

    let left_paths = session_a_paths.clone();
    let right_paths = session_b_paths.clone();
    let left = thread::spawn(move || run_isolated_tab("A", left_paths));
    let right = thread::spawn(move || run_isolated_tab("B", right_paths));
    transcript.push_str("Parallel session isolation:\n");
    transcript.push_str(&format!("  {}\n", left.join().expect("session A thread")?));
    transcript.push_str(&format!(
        "  {}\n\n",
        right.join().expect("session B thread")?
    ));

    let cancellation = CancellationToken::new();
    let worker_token = cancellation.clone();
    let worker = thread::spawn(move || {
        let mut session = RSession::new()?;
        session.eval_result_cancellable("repeat { 1 + 1 }", &worker_token)
    });
    thread::sleep(Duration::from_millis(10));
    cancellation.cancel();
    let cancelled = worker
        .join()
        .expect("cancellation worker")
        .expect_err("long-running eval should be cancelled");
    transcript.push_str("Cancellation:\n");
    transcript.push_str(&format!("  {}\n", cancelled));

    let transcript_path = out_dir.join("showcase-transcript.txt");
    std::fs::write(&transcript_path, transcript)?;
    println!("Wrote {}", transcript_path.display());
    println!("Wrote {}", line_plot_path.display());
    println!("Wrote {}", point_plot_path.display());

    Ok(())
}

fn runtime_paths(
    out_dir: &Path,
    session_name: &str,
    library_dir: &Path,
) -> Result<AndroidRuntimePaths, Box<dyn Error + Send + Sync>> {
    let app_dir = out_dir.join(session_name).join("files");
    let cache_dir = out_dir.join(session_name).join("cache");
    std::fs::create_dir_all(&app_dir)?;
    std::fs::create_dir_all(&cache_dir)?;
    Ok(AndroidRuntimePaths::new(
        app_dir.to_string_lossy().into_owned(),
        cache_dir.to_string_lossy().into_owned(),
        Some(library_dir.to_string_lossy().into_owned()),
    ))
}

fn run_isolated_tab(
    label: &str,
    paths: AndroidRuntimePaths,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let mut session = RSession::new()?;
    session.configure_android_runtime(&paths)?;
    session.eval_result(&format!(r#"tab_value <- "{label}""#))?;
    let output = session.eval_result("tab_value")?.output;
    let missing_other = session.eval_result("exists(\"other_tab_value\")")?.output;
    Ok(format!(
        "Session {label}: tab_value={}, other_tab_value visible={}",
        output.trim(),
        missing_other.trim()
    ))
}

fn write_demo_package(package_dir: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    let r_dir = package_dir.join("R");
    std::fs::create_dir_all(&r_dir)?;
    std::fs::write(
        package_dir.join("DESCRIPTION"),
        "\
Package: androiddemo
Version: 0.1.0
Title: RPort Android Demo Package
Description: Pure-R package bundled with the Android showcase.
License: MIT
Encoding: UTF-8
NeedsCompilation: no
",
    )?;
    std::fs::write(
        package_dir.join("NAMESPACE"),
        "\
export(demo_value, demo_object, demo_label)
S3method(demo_label,androiddemo)
",
    )?;
    std::fs::write(
        r_dir.join("demo.R"),
        r#"
demo_value <- function(x = 41) x + 1
demo_object <- function(name = "android") { x <- 1L; class(x) <- "androiddemo"; x }
demo_label <- function(x) UseMethod("demo_label", x)
demo_label.androiddemo <- function(x) "S3 dispatch: androiddemo"
"#,
    )?;
    Ok(())
}

fn describe_value(value: &RValue) -> String {
    match value {
        RValue::Null => "NULL".to_string(),
        RValue::Logical(value) => format!("logical scalar {value:?}"),
        RValue::Integer(value) => format!("integer scalar {value:?}"),
        RValue::Real(value) => format!("real scalar {value:?}"),
        RValue::LogicalVector(values) => format!("logical vector len={}", values.len()),
        RValue::IntegerVector(values) => format!("integer vector {values:?}"),
        RValue::RealVector(values) => format!("real vector {values:?}"),
        RValue::StringVector(values) => format!("string vector {values:?}"),
        RValue::RawVector(values) => format!("raw vector len={}", values.len()),
        RValue::ComplexVector(values) => format!("complex vector len={}", values.len()),
        RValue::List(values) => format!("list len={}", values.len()),
        RValue::Attributed { value, metadata } => {
            format!(
                "attributed({}, class={:?}, names={:?})",
                describe_value(value),
                metadata.class,
                metadata.names
            )
        }
        RValue::Unsupported { type_name } => format!("unsupported {type_name}"),
        RValue::Error(message) => format!("error {message}"),
    }
}
