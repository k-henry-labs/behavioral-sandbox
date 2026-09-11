//! The cross-file lint the compiler cannot express.
//!
//! A workflow names repo paths as bare shell text, and the prose-drift lint reads `.rs` and `.md`
//! only, so a rename lands green here and fails days later on a scheduled job. An untagged fence in
//! the book is the same shape of failure: green locally, and the site stops deploying.
//!
//! It runs under `cargo xtask ci` like any other test.

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::workspace_root;

    /// Every fenced block in the book carries a language tag.
    ///
    /// **rustdoc compiles an untagged fence as Rust**, so one takes `mdbook test` down with it and
    /// the Docs workflow deploys nothing. Measured: a fence of probe output in `architecture.md`
    /// left the book undeployed from 7894540 until it was found. Checked here rather than by
    /// running `mdbook`, because the gate must need no tool the workflow installs for itself.
    #[test]
    fn every_book_fence_is_tagged() {
        let docs = workspace_root().join("docs");
        let mut untagged: Vec<String> = Vec::new();
        let mut fences = 0usize;
        let entries = std::fs::read_dir(&docs).expect("the book directory");
        let mut pages: Vec<std::path::PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "md"))
            .collect();
        pages.sort();
        assert!(!pages.is_empty(), "no book pages found under {docs:?}");

        for page in &pages {
            let text = std::fs::read_to_string(page).expect("a readable page");
            let mut open = false;
            for (idx, line) in text.lines().enumerate() {
                let Some(rest) = line.strip_prefix("```") else {
                    continue;
                };
                // A closing fence carries no tag, so only an opener is judged.
                if open {
                    open = false;
                    continue;
                }
                open = true;
                fences += 1;
                if rest.trim().is_empty() {
                    let name = page.file_name().unwrap_or_default().to_string_lossy();
                    untagged.push(format!("{name}:{}", idx + 1));
                }
            }
            assert!(!open, "unclosed fence in {page:?}");
        }
        assert!(fences > 0, "no fences found, so this lint proves nothing");
        assert!(
            untagged.is_empty(),
            "untagged code fence(s); rustdoc compiles these as Rust and `mdbook test` fails, \
             which stops the book deploying. Tag them (```text, ```console, ```rust): {untagged:?}"
        );
    }

    /// Workflows name repo files as bare shell text, which the prose-drift lint does not read, so
    /// a rename lands green and the weekly job fails days later.
    ///
    /// Scoped to the `crates/` and `xtask/` prefixes; a fetched URL and `dist/` are not ours.
    #[test]
    fn workflow_repo_paths_exist() {
        let repo = workspace_root();
        let mut checked = 0usize;
        let mut missing: Vec<String> = Vec::new();
        for (wf, text) in workflow_texts(repo) {
            for (idx, line) in text.lines().enumerate() {
                for token in line.split(|c: char| c.is_ascii_whitespace() || "\"'`(),".contains(c))
                {
                    if !(token.starts_with("crates/") || token.starts_with("xtask/")) {
                        continue;
                    }
                    // `crates/foo/**` is a path *filter*, not a file: check the dir it roots.
                    // Trailing sentence punctuation is not part of the path either.
                    let target = token
                        .trim_end_matches("/**")
                        .trim_end_matches(['.', ':', ';']);
                    checked += 1;
                    if !repo.join(target).exists() {
                        missing.push(format!("{wf}:{}: {target}", idx + 1));
                    }
                }
            }
        }
        // A workflow rename would otherwise leave the scan matching nothing and passing green.
        assert!(
            checked > 0,
            "no crates/ or xtask/ path reference matched in .github/workflows: the workflows no \
             longer name repo files the way this scan looks for, so it is asserting nothing"
        );
        assert!(
            missing.is_empty(),
            "workflow(s) reference repo paths that no longer exist:\n  {}",
            missing.join("\n  ")
        );
    }

    /// Every workflow file with its text, in name order, read from the directory rather than a
    /// list that would exempt what it omits. Both GitHub spellings; an empty directory fails.
    fn workflow_texts(repo: &Path) -> Vec<(String, String)> {
        let dir = repo.join(".github/workflows");
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .expect(".github/workflows")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("yml" | "yaml")))
            .collect();
        paths.sort();
        assert!(!paths.is_empty(), "no workflows found in {}", dir.display());
        paths
            .into_iter()
            .map(|p| {
                let wf = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let text = std::fs::read_to_string(&p).expect("read workflow");
                (wf, text)
            })
            .collect()
    }

    /// The supervisor writes the helper's argv and the CLI parses it, with no dependency between
    /// them, so only a boot would notice a rename. Compares spellings, which is what must agree.
    #[test]
    fn the_helper_flags_match_the_parser() {
        let repo = workspace_root();
        let writer = std::fs::read_to_string(repo.join("crates/supervisor/src/lib.rs"))
            .expect("crates/supervisor/src/lib.rs");
        let parser = std::fs::read_to_string(repo.join("crates/cli/src/vmm.rs"))
            .expect("crates/cli/src/vmm.rs");

        // What the supervisor pushes: every `"--flag".into()` in `helper_argv`.
        let written = flags(&writer, |line| line.contains(".into()"));
        assert!(
            written.len() >= 8,
            "expected the helper's flag set, found {written:?}"
        );
        // clap's explicit `long` spellings plus the ones it derives: `frame_log` is `--frame-log`.
        let mut missing = Vec::new();
        for flag in &written {
            let bare = flag.trim_start_matches('-');
            let field = bare.replace('-', "_");
            let declared_explicitly = parser.contains(&format!("long = \"{bare}\""));
            let derived_from_field = parser.contains(&format!("\n    pub(crate) {field}:"));
            if !declared_explicitly && !derived_from_field {
                missing.push(flag.clone());
            }
        }
        assert!(
            missing.is_empty(),
            "the supervisor writes {missing:?}, which crates/cli/src/vmm.rs does not parse"
        );
    }

    /// **Every posture flag `boxdesk run` takes is one the Go SDK knows.**
    ///
    /// Go is the one SDK still built on an argv: Python, JS and Rust call `execute_sandbox`
    /// directly, so a renamed field is a compile error for them and no lint is needed. Go's
    /// options are typed out by hand against a command line, which a flag added to the CLI moves
    /// out from under — its tests feed a stub binary, so the suite stays green while the thing it
    /// wraps has changed. That is exactly how `--keep` arrived and sat unknown to every SDK back
    /// when all four were wrappers.
    ///
    /// Matched on the flag as a QUOTED STRING, which is what an SDK building an argv writes. A
    /// looser match on the spelling anywhere in the source passes on a doc comment that merely
    /// mentions the flag — watched happen, with `--keep` deleted from the Go options and its own
    /// comment keeping the lint green.
    #[test]
    fn every_run_flag_is_one_the_sdks_know() {
        let repo = workspace_root();
        let parser = std::fs::read_to_string(repo.join("crates/cli/src/run.rs"))
            .expect("crates/cli/src/run.rs");

        // clap's explicit `long = "..."` spellings, else the one derived from the field name.
        // An explicit spelling WINS: the field `mounts` carries `long = "mount"`, and a lint that
        // took both would demand a `--mounts` nothing accepts.
        let mut taken: Vec<String> = Vec::new();
        let mut named_by_attribute: Option<String> = None;
        for line in parser.lines() {
            if line.trim_start().starts_with("#[arg(") {
                named_by_attribute = line.find("long = \"").and_then(|at| {
                    let rest = &line[at + 8..];
                    rest.find('"').map(|end| rest[..end].to_string())
                });
            }
            let Some(field) = line
                .strip_prefix("    pub(crate) ")
                .and_then(|f| f.split(':').next())
            else {
                continue;
            };
            let spelling = named_by_attribute
                .take()
                .unwrap_or_else(|| field.replace('_', "-"));
            taken.push(format!("--{spelling}"));
        }
        taken.sort();
        taken.dedup();
        // Not a posture an SDK offers. `--json` is how one reads anything at all and `--dry-run`
        // has its own method; `--screenshot` and `--frame-log` are the measurement flags the
        // benches drive, named nowhere in the posture table the book publishes.
        taken.retain(|f| {
            !matches!(
                f.as_str(),
                "--json" | "--dry-run" | "--command" | "--screenshot" | "--frame-log"
            )
        });
        assert!(
            taken.len() >= 10,
            "expected the run verb's flag set, found {taken:?}"
        );

        let sdks = [("go", "sdk/go")];
        let mut unknown: Vec<String> = Vec::new();
        for (name, dir) in sdks {
            let Ok(entries) = walk(&repo.join(dir)) else {
                continue;
            };
            let source: String = entries.join("\n");
            for flag in &taken {
                if !source.contains(&format!("\"{flag}\"")) {
                    unknown.push(format!("{name} does not pass {flag}"));
                }
            }
        }
        assert!(
            unknown.is_empty(),
            "`boxdesk run` takes flags the SDKs have never heard of:\n  {}",
            unknown.join("\n  ")
        );
    }

    /// Every source file under `dir`, read, for a lint that only wants to grep a tree.
    fn walk(dir: &Path) -> std::io::Result<Vec<String>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                out.extend(walk(&path)?);
            } else if path
                .extension()
                .is_some_and(|e| matches!(e.to_str(), Some("py" | "ts" | "go" | "rs")))
            {
                out.push(std::fs::read_to_string(&path)?);
            }
        }
        Ok(out)
    }

    /// Every `"--flag"` literal on a line the predicate accepts, deduplicated, in sorted order.
    fn flags(src: &str, keep: impl Fn(&str) -> bool) -> Vec<String> {
        let mut out: Vec<String> = src
            .lines()
            .filter(|l| keep(l))
            .filter_map(|l| {
                let start = l.find("\"--")?;
                let rest = &l[start + 1..];
                let end = rest.find('"')?;
                Some(rest[..end].to_string())
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }
}
