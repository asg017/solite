//! Snapshot assertion handling for the test command.

mod diff;
mod file;
pub mod value;

pub use diff::{print_decision, print_diff};
pub use file::generate_snapshot_contents;
pub use value::{copy, snapshot_value, ValueCopy, ValueCopyValue};

use console::{Key, Style, Term};
use solite_core::sqlite::Statement;
use std::collections::HashSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// The snapshot testing mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapMode {
    /// Fail on any mismatch or new snapshot (CI mode).
    Default,
    /// Auto-accept all changes (agent mode).
    Update,
    /// Interactive accept/reject per change.
    Review,
}

/// Tracks snapshot state across a test run.
pub struct SnapState {
    pub snapshots_dir: PathBuf,
    pub generated_snapshots: HashSet<String>,
    pub mode: SnapMode,
    pub matches: usize,
    pub new: usize,
    pub updated: usize,
    pub rejected: usize,
    pub removed: usize,
}

impl SnapState {
    pub fn new(test_file: &Path, mode: SnapMode) -> Self {
        let snapshots_dir = test_file
            .parent()
            .map(|p| p.join("__snapshots__"))
            .unwrap_or_else(|| PathBuf::from("__snapshots__"));

        Self {
            snapshots_dir,
            generated_snapshots: HashSet::new(),
            mode,
            matches: 0,
            new: 0,
            updated: 0,
            rejected: 0,
            removed: 0,
        }
    }

    pub fn has_failures(&self) -> bool {
        self.rejected > 0
    }

    pub fn has_snapshots(&self) -> bool {
        self.matches > 0 || self.new > 0 || self.updated > 0 || self.rejected > 0
    }

    /// Ensure the snapshots directory exists.
    pub fn ensure_dir(&self) -> Result<(), String> {
        if !self.snapshots_dir.exists() {
            std::fs::create_dir_all(&self.snapshots_dir).map_err(|e| {
                format!(
                    "Failed to create snapshots directory {}: {}",
                    self.snapshots_dir.display(),
                    e
                )
            })?;
        }
        Ok(())
    }

    /// Print snapshot summary as part of test results.
    pub fn print_summary(&self) {
        if !self.has_snapshots() && self.removed == 0 {
            return;
        }
        println!();
        if self.matches > 0 {
            println!(
                "{:>4} snapshot{} passed",
                self.matches,
                if self.matches == 1 { "" } else { "s" }
            );
        }
        if self.new > 0 {
            println!(
                "{:>4} snapshot{} created",
                self.new,
                if self.new == 1 { "" } else { "s" }
            );
        }
        if self.updated > 0 {
            println!(
                "{:>4} snapshot{} updated",
                self.updated,
                if self.updated == 1 { "" } else { "s" }
            );
        }
        if self.rejected > 0 {
            println!(
                "{:>4} snapshot{} rejected",
                self.rejected,
                if self.rejected == 1 { "" } else { "s" }
            );
        }
        if self.removed > 0 {
            println!(
                "{:>4} snapshot{} removed",
                self.removed,
                if self.removed == 1 { "" } else { "s" }
            );
        }
    }
}

/// Handle a `@snap <name>` assertion for a SQL statement.
///
/// Generates snapshot content, compares against the existing `.snap` file,
/// and creates/updates/rejects based on the mode.
pub fn handle_snap_assertion(
    state: &mut SnapState,
    stmt: &Statement,
    snap_name: &str,
    filestem: &str,
    source_path: &Path,
) {
    // Ensure snapshots directory exists
    if let Err(e) = state.ensure_dir() {
        eprintln!("{}", e);
        state.rejected += 1;
        print!("{}", Style::new().red().apply_to("x"));
        let _ = std::io::stdout().flush();
        return;
    }

    let snapshot_filename = format!("{}-{}.snap", filestem, snap_name);
    let snapshot_path = state.snapshots_dir.join(&snapshot_filename);
    state
        .generated_snapshots
        .insert(snapshot_filename.clone());

    // Compute relative source path from snapshots dir
    let source = pathdiff::diff_paths(source_path, &state.snapshots_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| source_path.to_string_lossy().to_string());

    let snapshot_contents = match generate_snapshot_contents(source, stmt) {
        Some(c) => c,
        None => {
            // Statement produced no snappable output (e.g. CREATE TABLE)
            // Still counts as a pass - the statement executed successfully
            state.matches += 1;
            print!("{}", Style::new().green().apply_to("."));
            let _ = std::io::stdout().flush();
            return;
        }
    };

    if snapshot_path.exists() {
        // Compare with existing snapshot
        let original = match std::fs::read_to_string(&snapshot_path) {
            Ok(s) => s.replace("\r\n", "\n"),
            Err(e) => {
                eprintln!(
                    "Failed to read snapshot {}: {}",
                    snapshot_path.display(),
                    e
                );
                state.rejected += 1;
                print!("{}", Style::new().red().apply_to("x"));
                let _ = std::io::stdout().flush();
                return;
            }
        };

        if original == snapshot_contents {
            state.matches += 1;
            print!("{}", Style::new().green().apply_to("."));
        } else {
            handle_mismatch(state, &snapshot_path, &original, &snapshot_contents);
        }
    } else {
        handle_new_snapshot(state, &snapshot_path, &snapshot_contents);
    }

    let _ = std::io::stdout().flush();
}

/// Handle a snapshot that differs from the existing file.
fn handle_mismatch(
    state: &mut SnapState,
    snapshot_path: &Path,
    original: &str,
    new_contents: &str,
) {
    match state.mode {
        SnapMode::Default => {
            println!("\nSnapshot mismatch: {}", snapshot_path.display());
            print_diff(original, new_contents);
            state.rejected += 1;
            print!("{}", Style::new().red().apply_to("x"));
        }
        SnapMode::Update => {
            if write_snapshot(snapshot_path, new_contents).is_ok() {
                println!("\nUpdated: {}", snapshot_path.display());
                state.updated += 1;
                print!("{}", Style::new().yellow().apply_to("u"));
            } else {
                state.rejected += 1;
                print!("{}", Style::new().red().apply_to("x"));
            }
        }
        SnapMode::Review => {
            println!("\nSnapshot changed: {}", snapshot_path.display());
            print_diff(original, new_contents);
            print_decision();

            let term = Term::stdout();
            match term.read_key() {
                Ok(Key::Char('a') | Key::Char('A') | Key::Enter) => {
                    if write_snapshot(snapshot_path, new_contents).is_ok() {
                        state.updated += 1;
                        print!("{}", Style::new().yellow().apply_to("u"));
                    } else {
                        state.rejected += 1;
                        print!("{}", Style::new().red().apply_to("x"));
                    }
                }
                _ => {
                    state.rejected += 1;
                    print!("{}", Style::new().red().apply_to("x"));
                }
            }
        }
    }
}

/// Handle a snapshot that doesn't exist yet.
fn handle_new_snapshot(state: &mut SnapState, snapshot_path: &Path, contents: &str) {
    match state.mode {
        SnapMode::Default => {
            println!("\nNew snapshot: {}", snapshot_path.display());
            print_diff("", contents);
            state.rejected += 1;
            print!("{}", Style::new().red().apply_to("x"));
        }
        SnapMode::Update => {
            if write_snapshot(snapshot_path, contents).is_ok() {
                println!("\nCreated: {}", snapshot_path.display());
                state.new += 1;
                print!("{}", Style::new().green().apply_to("+"));
            } else {
                state.rejected += 1;
                print!("{}", Style::new().red().apply_to("x"));
            }
        }
        SnapMode::Review => {
            println!("\nNew snapshot: {}", snapshot_path.display());
            print_diff("", contents);
            print_decision();

            let term = Term::stdout();
            match term.read_key() {
                Ok(Key::Char('a') | Key::Char('A') | Key::Enter) => {
                    if write_snapshot(snapshot_path, contents).is_ok() {
                        state.new += 1;
                        print!("{}", Style::new().green().apply_to("+"));
                    } else {
                        state.rejected += 1;
                        print!("{}", Style::new().red().apply_to("x"));
                    }
                }
                _ => {
                    state.rejected += 1;
                    print!("{}", Style::new().red().apply_to("x"));
                }
            }
        }
    }
}

/// Check for orphaned snapshot files and handle based on mode.
pub fn handle_orphans(state: &mut SnapState, filestem: &str) {
    if !state.snapshots_dir.exists() {
        return;
    }

    let entries = match std::fs::read_dir(&state.snapshots_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let prefix = format!("{}-", filestem);
    let mut orphans: Vec<String> = Vec::new();

    for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str() {
            if name.starts_with(&prefix)
                && name.ends_with(".snap")
                && !state.generated_snapshots.contains(name)
            {
                orphans.push(name.to_string());
            }
        }
    }

    orphans.sort();

    for orphan in &orphans {
        let path = state.snapshots_dir.join(orphan);
        match state.mode {
            SnapMode::Default => {
                eprintln!("Warning: orphaned snapshot: {}", path.display());
            }
            SnapMode::Update => {
                if let Err(e) = std::fs::remove_file(&path) {
                    eprintln!("Failed to remove orphan {}: {}", path.display(), e);
                } else {
                    println!("Removed orphan: {}", path.display());
                    state.removed += 1;
                }
            }
            SnapMode::Review => {
                let contents = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                print_diff(&contents, "");
                println!("Remove {}? [y/n]", path.display());

                let term = Term::stdout();
                match term.read_key() {
                    Ok(Key::Char('y') | Key::Char('Y')) => {
                        if let Err(e) = std::fs::remove_file(&path) {
                            eprintln!("Failed to remove {}: {}", path.display(), e);
                        } else {
                            state.removed += 1;
                        }
                    }
                    _ => {
                        println!("Keeping {}", path.display());
                    }
                }
            }
        }
    }
}

/// Write snapshot contents to a file (create or overwrite).
pub(crate) fn write_snapshot(path: &Path, contents: &str) -> Result<(), ()> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path);

    match file {
        Ok(mut f) => {
            if let Err(e) = f.write_all(contents.as_bytes()) {
                eprintln!("Failed to write snapshot {}: {}", path.display(), e);
                return Err(());
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to open snapshot {}: {}", path.display(), e);
            Err(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "solite-snap-test-{}-{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    // --- SnapState constructor tests ---

    #[test]
    fn snap_state_new_sets_snapshots_dir() {
        let state = SnapState::new(Path::new("/foo/bar/test.sql"), SnapMode::Default);
        assert_eq!(state.snapshots_dir, PathBuf::from("/foo/bar/__snapshots__"));
    }

    #[test]
    fn snap_state_new_nested_path() {
        let state = SnapState::new(Path::new("/a/b/c/d/test.sql"), SnapMode::Update);
        assert_eq!(state.snapshots_dir, PathBuf::from("/a/b/c/d/__snapshots__"));
    }

    #[test]
    fn snap_state_new_root_file() {
        let state = SnapState::new(Path::new("/test.sql"), SnapMode::Default);
        assert_eq!(state.snapshots_dir, PathBuf::from("/__snapshots__"));
    }

    #[test]
    fn snap_state_new_relative_file() {
        let state = SnapState::new(Path::new("tests/test.sql"), SnapMode::Default);
        assert_eq!(state.snapshots_dir, PathBuf::from("tests/__snapshots__"));
    }

    #[test]
    fn snap_state_new_counters_start_at_zero() {
        let state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        assert_eq!(state.matches, 0);
        assert_eq!(state.new, 0);
        assert_eq!(state.updated, 0);
        assert_eq!(state.rejected, 0);
        assert_eq!(state.removed, 0);
    }

    #[test]
    fn snap_state_new_empty_generated() {
        let state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        assert!(state.generated_snapshots.is_empty());
    }

    #[test]
    fn snap_state_preserves_mode() {
        let s1 = SnapState::new(Path::new("t.sql"), SnapMode::Default);
        let s2 = SnapState::new(Path::new("t.sql"), SnapMode::Update);
        let s3 = SnapState::new(Path::new("t.sql"), SnapMode::Review);
        assert_eq!(s1.mode, SnapMode::Default);
        assert_eq!(s2.mode, SnapMode::Update);
        assert_eq!(s3.mode, SnapMode::Review);
    }

    // --- has_failures tests ---

    #[test]
    fn has_failures_false_initially() {
        let state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        assert!(!state.has_failures());
    }

    #[test]
    fn has_failures_true_when_rejected() {
        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.rejected = 1;
        assert!(state.has_failures());
    }

    #[test]
    fn has_failures_false_when_only_matches() {
        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.matches = 5;
        assert!(!state.has_failures());
    }

    #[test]
    fn has_failures_false_when_updated() {
        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.updated = 3;
        assert!(!state.has_failures());
    }

    #[test]
    fn has_failures_false_when_new() {
        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.new = 2;
        assert!(!state.has_failures());
    }

    // --- has_snapshots tests ---

    #[test]
    fn has_snapshots_false_initially() {
        let state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        assert!(!state.has_snapshots());
    }

    #[test]
    fn has_snapshots_true_with_matches() {
        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.matches = 1;
        assert!(state.has_snapshots());
    }

    #[test]
    fn has_snapshots_true_with_new() {
        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.new = 1;
        assert!(state.has_snapshots());
    }

    #[test]
    fn has_snapshots_true_with_updated() {
        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.updated = 1;
        assert!(state.has_snapshots());
    }

    #[test]
    fn has_snapshots_true_with_rejected() {
        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.rejected = 1;
        assert!(state.has_snapshots());
    }

    #[test]
    fn has_snapshots_false_with_only_removed() {
        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.removed = 3;
        assert!(!state.has_snapshots());
    }

    // --- ensure_dir tests ---

    #[test]
    fn ensure_dir_creates_directory() {
        let tmp = temp_dir();
        let snap_dir = tmp.join("__snapshots__");
        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.snapshots_dir = snap_dir.clone();

        assert!(!snap_dir.exists());
        state.ensure_dir().unwrap();
        assert!(snap_dir.exists());
        assert!(snap_dir.is_dir());

        cleanup(&tmp);
    }

    #[test]
    fn ensure_dir_ok_when_already_exists() {
        let tmp = temp_dir();
        let snap_dir = tmp.join("__snapshots__");
        fs::create_dir_all(&snap_dir).unwrap();

        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.snapshots_dir = snap_dir;

        assert!(state.ensure_dir().is_ok());

        cleanup(&tmp);
    }

    #[test]
    fn ensure_dir_creates_nested_dirs() {
        let tmp = temp_dir();
        let snap_dir = tmp.join("a").join("b").join("__snapshots__");
        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.snapshots_dir = snap_dir.clone();

        state.ensure_dir().unwrap();
        assert!(snap_dir.exists());

        cleanup(&tmp);
    }

    // --- write_snapshot tests ---

    #[test]
    fn write_snapshot_creates_file() {
        let tmp = temp_dir();
        let path = tmp.join("test.snap");

        write_snapshot(&path, "hello world").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");

        cleanup(&tmp);
    }

    #[test]
    fn write_snapshot_overwrites_existing() {
        let tmp = temp_dir();
        let path = tmp.join("test.snap");

        fs::write(&path, "old content").unwrap();
        write_snapshot(&path, "new content").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new content");

        cleanup(&tmp);
    }

    #[test]
    fn write_snapshot_handles_empty_content() {
        let tmp = temp_dir();
        let path = tmp.join("empty.snap");

        write_snapshot(&path, "").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "");

        cleanup(&tmp);
    }

    #[test]
    fn write_snapshot_handles_multiline() {
        let tmp = temp_dir();
        let path = tmp.join("multi.snap");
        let content = "Source: test.sql\nSELECT 1;\n---\n1\n";

        write_snapshot(&path, content).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), content);

        cleanup(&tmp);
    }

    // --- handle_orphans tests ---

    #[test]
    fn handle_orphans_no_dir_does_nothing() {
        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.snapshots_dir = PathBuf::from("/nonexistent/path/__snapshots__");
        // Should not panic
        handle_orphans(&mut state, "test");
        assert_eq!(state.removed, 0);
    }

    #[test]
    fn handle_orphans_no_orphans() {
        let tmp = temp_dir();
        let snap_dir = tmp.join("__snapshots__");
        fs::create_dir_all(&snap_dir).unwrap();

        // Create a snapshot that IS tracked
        fs::write(snap_dir.join("test-my-snap.snap"), "content").unwrap();

        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Update);
        state.snapshots_dir = snap_dir;
        state.generated_snapshots.insert("test-my-snap.snap".to_string());

        handle_orphans(&mut state, "test");
        assert_eq!(state.removed, 0);

        cleanup(&tmp);
    }

    #[test]
    fn handle_orphans_default_mode_warns_but_doesnt_delete() {
        let tmp = temp_dir();
        let snap_dir = tmp.join("__snapshots__");
        fs::create_dir_all(&snap_dir).unwrap();

        let orphan_path = snap_dir.join("test-old-snap.snap");
        fs::write(&orphan_path, "orphan content").unwrap();

        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Default);
        state.snapshots_dir = snap_dir;
        // No generated snapshots → orphan_path is orphaned

        handle_orphans(&mut state, "test");

        // Should still exist (default mode only warns)
        assert!(orphan_path.exists());
        assert_eq!(state.removed, 0);

        cleanup(&tmp);
    }

    #[test]
    fn handle_orphans_update_mode_deletes() {
        let tmp = temp_dir();
        let snap_dir = tmp.join("__snapshots__");
        fs::create_dir_all(&snap_dir).unwrap();

        let orphan_path = snap_dir.join("test-old-snap.snap");
        fs::write(&orphan_path, "orphan content").unwrap();

        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Update);
        state.snapshots_dir = snap_dir;

        handle_orphans(&mut state, "test");

        assert!(!orphan_path.exists());
        assert_eq!(state.removed, 1);

        cleanup(&tmp);
    }

    #[test]
    fn handle_orphans_update_deletes_multiple() {
        let tmp = temp_dir();
        let snap_dir = tmp.join("__snapshots__");
        fs::create_dir_all(&snap_dir).unwrap();

        fs::write(snap_dir.join("test-a.snap"), "a").unwrap();
        fs::write(snap_dir.join("test-b.snap"), "b").unwrap();
        fs::write(snap_dir.join("test-c.snap"), "c").unwrap();

        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Update);
        state.snapshots_dir = snap_dir;

        handle_orphans(&mut state, "test");
        assert_eq!(state.removed, 3);

        cleanup(&tmp);
    }

    #[test]
    fn handle_orphans_ignores_non_matching_prefix() {
        let tmp = temp_dir();
        let snap_dir = tmp.join("__snapshots__");
        fs::create_dir_all(&snap_dir).unwrap();

        // This file has a different prefix
        let other_file = snap_dir.join("other-snap.snap");
        fs::write(&other_file, "content").unwrap();

        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Update);
        state.snapshots_dir = snap_dir;

        handle_orphans(&mut state, "test");

        // Should NOT be deleted — different prefix
        assert!(other_file.exists());
        assert_eq!(state.removed, 0);

        cleanup(&tmp);
    }

    #[test]
    fn handle_orphans_ignores_non_snap_extension() {
        let tmp = temp_dir();
        let snap_dir = tmp.join("__snapshots__");
        fs::create_dir_all(&snap_dir).unwrap();

        // Has the right prefix but wrong extension
        fs::write(snap_dir.join("test-foo.txt"), "content").unwrap();

        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Update);
        state.snapshots_dir = snap_dir;

        handle_orphans(&mut state, "test");
        assert_eq!(state.removed, 0);

        cleanup(&tmp);
    }

    #[test]
    fn handle_orphans_keeps_tracked_deletes_untracked() {
        let tmp = temp_dir();
        let snap_dir = tmp.join("__snapshots__");
        fs::create_dir_all(&snap_dir).unwrap();

        fs::write(snap_dir.join("test-keep.snap"), "keep").unwrap();
        fs::write(snap_dir.join("test-delete.snap"), "delete").unwrap();

        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Update);
        state.snapshots_dir = snap_dir.clone();
        state.generated_snapshots.insert("test-keep.snap".to_string());

        handle_orphans(&mut state, "test");

        assert!(snap_dir.join("test-keep.snap").exists());
        assert!(!snap_dir.join("test-delete.snap").exists());
        assert_eq!(state.removed, 1);

        cleanup(&tmp);
    }

    #[test]
    fn handle_orphans_empty_directory() {
        let tmp = temp_dir();
        let snap_dir = tmp.join("__snapshots__");
        fs::create_dir_all(&snap_dir).unwrap();

        let mut state = SnapState::new(Path::new("test.sql"), SnapMode::Update);
        state.snapshots_dir = snap_dir;

        handle_orphans(&mut state, "test");
        assert_eq!(state.removed, 0);

        cleanup(&tmp);
    }
}
