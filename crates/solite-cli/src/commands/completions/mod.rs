//! Shell completion support.
//!
//! Solite uses `clap_complete`'s **dynamic** completion engine (the
//! `unstable-dynamic` feature): the `solite` binary is re-invoked by the shell
//! at TAB time (the `COMPLETE` env var is set) and emits candidates from Rust.
//! This is what lets later completers list procedure names and SQL keywords —
//! things a static, pre-generated completion script could never produce.
//!
//! Two halves live here:
//! - The [`completions`] entry point implements the `solite completions <shell>`
//!   subcommand, which prints the one-time per-shell registration snippet the
//!   user adds to their rc file.
//! - The `CompleteEnv` hook in [`crate::run_main`] does the actual candidate
//!   generation; the value-completer helpers it relies on are added to this
//!   module by later tickets (`files.rs`, `procedures.rs`, `sql.rs`).

use std::io::Write;

use clap_complete::env::Shells;

use crate::cli::CompletionsArgs;

pub(crate) mod files;
pub(crate) mod procedures;

/// Print the dynamic-completion registration script for the requested shell.
///
/// The output is meant to be sourced from the user's shell rc file, e.g.
/// `source <(solite completions zsh)`. It registers a hook that sets
/// `COMPLETE=<shell>` and re-invokes `solite` to produce candidates.
pub fn completions(args: CompletionsArgs) -> Result<(), ()> {
    // `clap_complete::Shell` (the value-enum used by the arg) renders to the
    // same canonical names the dynamic `EnvCompleter`s match on ("bash", "zsh",
    // …), so look the completer up by that name.
    let name = args.shell.to_string();
    let shells = Shells::builtins();
    let completer = shells.completer(&name).ok_or_else(|| {
        eprintln!("Unsupported shell for completions: {name}");
    })?;

    // The binary is always invoked as `solite`; use that for both the script
    // identifier and the completer command.
    let bin = "solite";
    let mut buf = Vec::new();
    completer
        .write_registration("COMPLETE", bin, bin, bin, &mut buf)
        .map_err(|e| {
            eprintln!("Failed to generate completion script: {e}");
        })?;

    std::io::stdout().write_all(&buf).map_err(|e| {
        eprintln!("Failed to write completion script: {e}");
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use clap_complete::engine::complete;

    use crate::cli::command_for_completion;

    /// Drive the dynamic engine directly (no shell needed) and return the
    /// candidate strings for the given argv at `arg_index`. This is the
    /// deterministic completion harness later tickets reuse for file/procedure/
    /// SQL completers.
    pub(crate) fn complete_strings(argv: &[&str], arg_index: usize) -> Vec<String> {
        let mut cmd = command_for_completion();
        let args: Vec<OsString> = argv.iter().map(OsString::from).collect();
        complete(&mut cmd, args, arg_index, None)
            .expect("completion engine should not error")
            .into_iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn completes_top_level_subcommands() {
        // `solite <TAB>` should offer the documented subcommands.
        let candidates = complete_strings(&["solite", ""], 1);
        // Note: aliased commands surface under their visible alias here
        // (`query`→`q`, `execute`→`exec`), so assert on non-aliased names.
        for expected in ["run", "repl", "test", "completions"] {
            assert!(
                candidates.iter().any(|c| c == expected),
                "expected subcommand `{expected}` in {candidates:?}"
            );
        }
    }

    #[test]
    fn completes_subcommand_prefix() {
        // `solite ru<TAB>` should narrow to `run`.
        let candidates = complete_strings(&["solite", "ru"], 1);
        assert!(
            candidates.iter().any(|c| c == "run"),
            "expected `run` in {candidates:?}"
        );
    }
}
