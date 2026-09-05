//! Explicit provider accounting diagnostic:
//! `cargo run -p wardian-core --example verify_provider_log -- <codex|pi> <file>`.
//! All comparisons use one owned snapshot; no home or session discovery occurs.

#[path = "../tests/common/provider_log.rs"]
mod provider_log;

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use provider_log::{ProviderLog, Totals, VerifyError};

fn run(args: impl IntoIterator<Item = OsString>) -> Result<Totals, VerifyError> {
    let mut args = args.into_iter();
    let (Some(provider), Some(path), None) = (args.next(), args.next(), args.next()) else {
        return Err(VerifyError::Invalid(
            "usage: verify_provider_log <codex|pi> <file>",
        ));
    };
    let provider = provider
        .to_str()
        .ok_or(VerifyError::Invalid("provider must be codex or pi"))?;
    ProviderLog::capture(provider, Path::new(&path))?.verify()
}

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1)) {
        Ok((input, cached, writes, output, reasoning)) => {
            println!("verified snapshot: input={input} cached={cached} cache_write={writes} output={output} reasoning={reasoning}; activity, rollups, and forced reparse agree");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("provider log verification failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    const CODEX: &str = include_str!("../tests/fixtures/codex-rollout.jsonl");
    const PI: &str = include_str!("../tests/fixtures/pi-session.jsonl");

    fn fixture(provider: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(match provider {
                "codex" => "codex-rollout.jsonl",
                _ => "pi-session.jsonl",
            })
    }

    fn check(provider: &str, path: &Path) -> Result<Totals, VerifyError> {
        run([OsString::from(provider), path.as_os_str().to_owned()])
    }

    #[test]
    fn required_arguments_and_provider_are_validated() {
        for args in [vec![], vec!["codex"], vec!["pi", "file", "extra"]] {
            let error = run(args.into_iter().map(OsString::from)).unwrap_err();
            assert!(error.to_string().contains("usage:"));
        }
        assert!(matches!(
            check("claude", &fixture("codex")),
            Err(VerifyError::Invalid("provider must be codex or pi"))
        ));
    }

    #[test]
    fn missing_unreadable_empty_and_invalid_inputs_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        for provider in ["codex", "pi"] {
            assert!(matches!(
                check(provider, &dir.path().join("missing.jsonl")),
                Err(VerifyError::Io(_))
            ));
            // A directory is never a readable log, even under an elevated account.
            assert!(matches!(
                check(provider, dir.path()),
                Err(VerifyError::Io(_))
            ));
            let path = dir.path().join("invalid.jsonl");
            for contents in [
                "",
                " \n",
                "not json\n",
                "{}\n",
                "[]\n",
                "{\"type\":\"session\"}\n",
            ] {
                std::fs::write(&path, contents).unwrap();
                assert!(
                    check(provider, &path).is_err(),
                    "{provider} accepted {contents:?}"
                );
            }
            std::fs::write(&path, [0xff, 0xfe]).unwrap();
            assert!(matches!(check(provider, &path), Err(VerifyError::Io(_))));
            // Malformation after valid accounting must not be silently discarded.
            let valid = if provider == "codex" { CODEX } else { PI };
            std::fs::write(&path, format!("{valid}\n{{broken\n")).unwrap();
            assert!(matches!(
                check(provider, &path),
                Err(VerifyError::Json { .. })
            ));
        }
    }

    #[test]
    fn example_verifies_both_committed_fixtures() {
        assert_eq!(
            check("codex", &fixture("codex")).unwrap(),
            (100_544, 730_880, 0, 5_254, 2_244)
        );
        assert_eq!(
            check("pi", &fixture("pi")).unwrap(),
            (20_640, 7_680, 0, 183, 98)
        );
    }

    #[test]
    fn broken_accounting_and_wrong_provider_fail() {
        assert!(check("pi", &fixture("codex")).is_err());
        assert!(check("codex", &fixture("pi")).is_err());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad-accounting.jsonl");
        for (provider, text) in [
            ("codex", CODEX.replace("831424", "831425")),
            ("pi", PI.replace("\"input\":8586", "\"input\":8587")),
            ("pi", PI.replace("\"cacheWrite\":0", "\"cacheWrite\":1")),
            ("pi", PI.replace("\"input\":8586", "\"input\":-1")),
            (
                "pi",
                PI.replace("\"input\":8586", "\"input\":9223372036854775807"),
            ),
        ] {
            std::fs::write(&path, text).unwrap();
            assert!(
                check(provider, &path).is_err(),
                "{provider} accepted broken accounting"
            );
        }
    }

    #[test]
    fn snapshot_survives_source_append_and_removal() {
        let dir = tempfile::tempdir().unwrap();
        for (provider, initial, append) in [
            ("codex", CODEX, concat!(
                "{\"timestamp\":\"2026-08-13T15:03:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{",
                "\"total_token_usage\":{\"input_tokens\":831425,\"cached_input_tokens\":730880,\"output_tokens\":5254,\"reasoning_output_tokens\":2244},",
                "\"last_token_usage\":{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":0,\"reasoning_output_tokens\":0}}}}\n")),
            ("pi", PI, concat!(
                "{\"type\":\"message\",\"id\":\"appended\",\"timestamp\":\"2026-08-24T04:34:25Z\",",
                "\"message\":{\"role\":\"assistant\",\"model\":\"gpt-5.6-luna\",\"content\":[],",
                "\"usage\":{\"input\":1,\"output\":0,\"cacheRead\":0,\"cacheWrite\":0,\"reasoning\":0,\"totalTokens\":1}}}\n")),
        ] {
            let path = dir.path().join(format!("{provider}.jsonl"));
            std::fs::write(&path, initial).unwrap();
            let snapshot = ProviderLog::capture(provider, &path).unwrap();
            let before = snapshot.verify().unwrap();

            let mut writer = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(writer, "{append}").unwrap();
            drop(writer);
            // Deterministically exhibit the old comparison's changed input:
            // reopening the source now reports one more fresh token.
            let after = check(provider, &path).unwrap();
            assert_eq!(after, (before.0 + 1, before.1, before.2, before.3, before.4));
            assert_eq!(snapshot.verify().unwrap(), before);

            std::fs::remove_file(&path).unwrap();
            assert_eq!(snapshot.verify().unwrap(), before, "snapshot reopened its source");
        }
    }
}
