//! `llm-xtask report` reads the JUnit reports cargo-nextest wrote, one per run of the test phase,
//! prints the ten slowest tests of all of them, slowest first, and the time the runs took, and
//! fails when that time is over the 120 s bound. `llm-xtask test` ends with the same report.

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

/// A report as cargo-nextest writes one: the run's time on `testsuites`, each test's on its
/// `testcase`.
fn junit(run_seconds: f64, cases: &[(&str, &str, f64)]) -> String {
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites name=\"nextest-run\" tests=\"{}\" \
         failures=\"0\" errors=\"0\" uuid=\"00000000-0000-0000-0000-000000000000\" \
         timestamp=\"2026-11-23T10:00:00.000+00:00\" time=\"{run_seconds:.3}\">\n",
        cases.len()
    );
    for (suite, name, seconds) in cases {
        writeln!(
            xml,
            "    <testsuite name=\"{suite}\" tests=\"1\" disabled=\"0\" errors=\"0\" failures=\"0\">\n        \
             <testcase name=\"{name}\" classname=\"{suite}\" \
             timestamp=\"2026-11-23T10:00:00.000+00:00\" time=\"{seconds:.3}\">\n        \
             </testcase>\n    </testsuite>"
        )
        .unwrap();
    }
    xml.push_str("</testsuites>\n");
    xml
}

fn write(directory: &Path, name: &str, xml: &str) -> PathBuf {
    let path = directory.join(name);
    fs::write(&path, xml).expect("the report is written");
    path
}

fn report(paths: &[PathBuf]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_llm-xtask"))
        .arg("report")
        .args(paths)
        .output()
        .expect("llm-xtask runs")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_ten_slowest_tests_of_every_run_are_printed_slowest_first() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let first = write(
        directory.path(),
        "first.xml",
        &junit(
            20.0,
            &[
                ("llm-models::routes", "t01", 1.0),
                ("llm-models::routes", "t03", 3.0),
                ("llm-models::routes", "t05", 5.0),
                ("llm-models::routes", "t07", 7.0),
                ("llm-models::routes", "t09", 9.0),
                ("llm-models::routes", "t11_&lt;escaped&gt;", 11.0),
            ],
        ),
    );
    let second = write(
        directory.path(),
        "second.xml",
        &junit(
            15.5,
            &[
                ("llm-cost::pricing", "t02", 2.0),
                ("llm-cost::pricing", "t04", 4.0),
                ("llm-cost::pricing", "t06", 6.0),
                ("llm-cost::pricing", "t08", 8.0),
                ("llm-cost::pricing", "t10", 10.0),
                ("llm-cost::pricing", "t12", 12.0),
            ],
        ),
    );
    let output = report(&[first, second]);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}{}", text(&output.stderr));

    let listed: Vec<&str> = stdout
        .lines()
        .filter(|line| line.contains(" s  llm-"))
        .collect();
    let expected = [
        "12.000 s  llm-cost::pricing t12",
        "11.000 s  llm-models::routes t11_<escaped>",
        "10.000 s  llm-cost::pricing t10",
        "9.000 s  llm-models::routes t09",
        "8.000 s  llm-cost::pricing t08",
        "7.000 s  llm-models::routes t07",
        "6.000 s  llm-cost::pricing t06",
        "5.000 s  llm-models::routes t05",
        "4.000 s  llm-cost::pricing t04",
        "3.000 s  llm-models::routes t03",
    ];
    assert_eq!(listed.len(), expected.len(), "{stdout}");
    for (line, expected) in listed.iter().zip(expected) {
        assert_eq!(line.trim(), expected, "{stdout}");
    }
    assert!(
        stdout.contains("the runs took 35.5 s of their bound of 120 s"),
        "{stdout}"
    );
}

#[test]
fn runs_that_take_longer_than_120_s_fail_the_phase() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let case = [("llm-core::turn", "t", 0.5)];
    let at_bound = [
        write(directory.path(), "a.xml", &junit(60.0, &case)),
        write(directory.path(), "b.xml", &junit(60.0, &case)),
    ];
    let output = report(&at_bound);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));

    let over = [
        write(directory.path(), "c.xml", &junit(60.0, &case)),
        write(directory.path(), "d.xml", &junit(60.5, &case)),
    ];
    let output = report(&over);
    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stdout));
    assert!(
        text(&output.stderr).contains("the runs took 120.5 s, longer than their bound of 120 s"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn a_missing_or_unreadable_report_fails_the_phase() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let missing = directory.path().join("missing.xml");
    let output = report(std::slice::from_ref(&missing));
    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stdout));
    assert!(text(&output.stderr).contains("missing.xml"), "{}", text(&output.stderr));

    let no_run_time = write(directory.path(), "bare.xml", "<testsuites>\n</testsuites>\n");
    let output = report(&[no_run_time]);
    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stdout));
}
