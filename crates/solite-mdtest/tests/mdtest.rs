//! Test harness for mdtest
//!
//! Discovers and runs all .md test files in resources/mdtest/

use std::path::Path;

/// Info about a failed test for summary
struct FailedTest {
    name: String,
    relative_path: String,
    line: usize,
    failures: Vec<String>,
}

fn main() {
    let _args: Vec<String> = std::env::args().collect();
    let cwd = std::env::current_dir().expect("Failed to get current directory");
    let test_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("mdtest");

    if !test_dir.exists() {
        println!("Test directory not found: {}", test_dir.display());
        println!("Creating empty test directory...");
        std::fs::create_dir_all(&test_dir).expect("Failed to create test directory");
        println!("No tests to run. Add .md files to {}", test_dir.display());
        return;
    }

    // Check for filter
    let filter = std::env::var("MDTEST_FILTER").ok();

    println!("Running mdtest suite from {}", test_dir.display());
    if let Some(ref f) = filter {
        println!("Filter: {}", f);
    }
    println!();

    let mut total = 0;
    let mut passed = 0;
    let mut failed_tests: Vec<FailedTest> = Vec::new();

    for entry in std::fs::read_dir(&test_dir).expect("Failed to read test directory") {
        let entry = entry.expect("Failed to read entry");
        let path = entry.path();

        if path.extension().map(|e| e == "md").unwrap_or(false) {
            let file_name = path.file_name().unwrap().to_string_lossy();

            // Apply filter if set
            if let Some(ref f) = filter {
                if !file_name.contains(f) {
                    continue;
                }
            }

            println!("{}:", file_name);

            let content = std::fs::read_to_string(&path).expect("Failed to read file");
            // Use absolute path for source tracking, relative for display
            let full_path = path.canonicalize().unwrap_or_else(|_| path.clone());
            let full_path_str = full_path.to_string_lossy();
            let tests =
                solite_mdtest::parse_markdown(&content, &full_path_str).expect("Failed to parse");

            for test in tests {
                total += 1;

                // Apply filter to test names too
                if let Some(ref f) = filter {
                    if !test.name.contains(f) && !file_name.contains(f) {
                        continue;
                    }
                }

                // Count assertions for this test
                let assertion_count = test.assertions.len();

                match solite_mdtest::run_test(&test) {
                    Ok(result) => {
                        if result.passed {
                            passed += 1;
                            println!("  \x1b[32m✓\x1b[0m {} \x1b[90m({})\x1b[0m", test.name, assertion_count);
                        } else {
                            // Convert absolute path to relative
                            let relative_path = Path::new(&result.source_file)
                                .strip_prefix(&cwd)
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_else(|_| result.source_file.clone());

                            println!(
                                "  \x1b[31m✗\x1b[0m {} \x1b[90m({}) {}:{}\x1b[0m",
                                test.name, assertion_count, relative_path, result.source_line
                            );
                            for failure in &result.failures {
                                println!("    \x1b[31m{}\x1b[0m", failure);
                            }

                            failed_tests.push(FailedTest {
                                name: test.name.clone(),
                                relative_path,
                                line: result.source_line,
                                failures: result.failures.iter().map(|f| f.to_string()).collect(),
                            });
                        }
                    }
                    Err(e) => {
                        let relative_path = Path::new(&test.source_file)
                            .strip_prefix(&cwd)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| test.source_file.clone());

                        println!("  \x1b[31m✗\x1b[0m {} \x1b[90m({}) {}:{}\x1b[0m - Error: {}",
                            test.name, assertion_count, relative_path, test.source_line, e);

                        failed_tests.push(FailedTest {
                            name: test.name.clone(),
                            relative_path,
                            line: test.source_line,
                            failures: vec![format!("Error: {}", e)],
                        });
                    }
                }
            }
            println!();
        }
    }

    println!("----------------------------------------");
    println!(
        "Results: {} passed, {} failed, {} total",
        passed, failed_tests.len(), total
    );

    // Print summary of failed tests at the bottom
    if !failed_tests.is_empty() {
        println!();
        println!("\x1b[31mFailed tests:\x1b[0m");
        for ft in &failed_tests {
            println!("  \x1b[31m✗\x1b[0m {} \x1b[90m{}:{}\x1b[0m", ft.name, ft.relative_path, ft.line);
            for failure in &ft.failures {
                println!("    {}", failure);
            }
        }
        std::process::exit(1);
    }
}
