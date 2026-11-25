//! Shell integration (shell-integration spec): `DropTerm`'s own
//! emitter scripts — the OSC 133 prompt/command marks and the OSC 7
//! cwd report, written from the public specs alone (the clean-room
//! discipline: no bytes from any project's shell-integration text).
//! The scripts are embedded here and materialized under the cache
//! seat at spawn time; the injection decision (SI-6) is this
//! module's other half.

/// The bash half: the `--rcfile` wrapper — sources the user's
/// `~/.bashrc`, then chains the hooks around whatever it set.
const BASH_RCFILE: &str = include_str!("shell-integration/dropterm.bash");

/// The zsh chain: a bootstrap ZDOTDIR whose four startup files source
/// their originals from the preserved ZDOTDIR, `.zshrc` appending the
/// hooks behind the user's own.
const ZSH_ZSHENV: &str = include_str!("shell-integration/zsh/.zshenv");
const ZSH_ZPROFILE: &str = include_str!("shell-integration/zsh/.zprofile");
const ZSH_ZLOGIN: &str = include_str!("shell-integration/zsh/.zlogin");
const ZSH_ZSHRC: &str = include_str!("shell-integration/zsh/.zshrc");

/// The fish half: the init-command source — fish's own prompt/command
/// event hooks, installed after fish's config has run.
const FISH_EMITTER: &str = include_str!("shell-integration/dropterm.fish");

/// Every script the seat carries: its path under the seat and its
/// embedded bytes, in materialization order.
const SEAT_FILES: &[(&str, &str)] = &[
    ("dropterm.bash", BASH_RCFILE),
    ("zsh/.zshenv", ZSH_ZSHENV),
    ("zsh/.zprofile", ZSH_ZPROFILE),
    ("zsh/.zlogin", ZSH_ZLOGIN),
    ("zsh/.zshrc", ZSH_ZSHRC),
    ("dropterm.fish", FISH_EMITTER),
];

/// The integration's cache seat: `dropterm/shell-integration` under
/// the XDG cache dir (the paste cache's sibling pattern). `None`
/// when no cache dir resolves.
pub fn integration_cache_dir() -> Option<std::path::PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("dropterm").join("shell-integration"))
}

/// Write every script under `dir`: absent or stale bytes rewritten,
/// unchanged bytes skipped (a byte-compare, never a timestamp).
/// Returns how many files were written — the unchanged-skip pin's
/// observable.
fn materialize_into(dir: &std::path::Path) -> usize {
    let mut written = 0;
    for (rel, bytes) in SEAT_FILES {
        let path = dir.join(rel);
        let stands = std::fs::read(&path).is_ok_and(|standing| standing == bytes.as_bytes());
        if stands {
            continue;
        }
        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return written;
        }
        if std::fs::write(&path, bytes).is_err() {
            return written;
        }
        written += 1;
    }
    written
}

/// Materialize the seat (the spawn path's entry): every script
/// stands under the cache dir, fresh or verified. `None` when no
/// cache dir resolves or the seat does not fully stand — a missing
/// emitter must never half-integrate, so the caller skips injection
/// whole.
pub fn materialize() -> Option<std::path::PathBuf> {
    let dir = integration_cache_dir()?;
    materialize_into(&dir);
    let stands = SEAT_FILES.iter().all(|(rel, bytes)| {
        std::fs::read(dir.join(rel)).is_ok_and(|standing| standing == bytes.as_bytes())
    });
    stands.then_some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn seat_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dropterm-shell-integration-tests-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("test seat dir");
        dir
    }

    #[test]
    fn the_seat_carries_every_script_byte_exact() {
        // All six files land under the seat with the embedded bytes:
        // the bash rcfile, the fish emitter, and zsh's four-file
        // chain.
        let dir = seat_dir("fresh");
        assert_eq!(materialize_into(&dir), 6);
        for (rel, bytes) in SEAT_FILES {
            assert_eq!(
                std::fs::read(dir.join(rel)).expect("the file stands"),
                bytes.as_bytes(),
                "the seat's {rel} must be the embedded bytes"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unchanged_seat_is_not_rewritten() {
        // The byte-compare skip: a second pass over its own output
        // writes nothing.
        let dir = seat_dir("skip");
        materialize_into(&dir);
        assert_eq!(materialize_into(&dir), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_file_is_rebuilt() {
        // Stale bytes — an older DropTerm's script, a truncated
        // write — are rewritten in place; the fresh files around
        // them are skipped.
        let dir = seat_dir("stale");
        materialize_into(&dir);
        std::fs::write(dir.join("dropterm.bash"), "old bytes").expect("age the file");
        assert_eq!(materialize_into(&dir), 1);
        assert_eq!(
            std::fs::read(dir.join("dropterm.bash")).expect("the file stands"),
            BASH_RCFILE.as_bytes()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_script_carries_the_acd_and_osc7_shapes() {
        // The wire, one consistent terminator (BEL): each shell's
        // script carries the exact mark bytes — D with the code, A,
        // C, and the file:// URL — whatever quoting the shell's
        // hook needs around them (bash's PS0 substitution carries
        // double quotes; the rest single). The consumer's pins and
        // the emitters cannot drift apart silently.
        for script in [BASH_RCFILE, ZSH_ZSHRC, FISH_EMITTER] {
            assert!(script.contains(r"\033]133;A\007"), "the A mark");
            assert!(script.contains(r"\033]133;C\007"), "the C mark");
            assert!(
                script.contains(r"\033]133;D;%d\007"),
                "the D mark with its code"
            );
            assert!(
                script.contains(r"\033]7;file://%s%s\007"),
                "the OSC 7 cwd report"
            );
        }
    }

    #[test]
    fn the_user_hooks_chain_behind_the_report() {
        // Never fight the user's own setup: bash composes onto their
        // PROMPT_COMMAND, zsh's chain sources each original from the
        // preserved ZDOTDIR, and fish rides events (whose own
        // handlers always coexist).
        assert!(BASH_RCFILE.contains(
            "PROMPT_COMMAND=\"__dropterm_prompt_command${PROMPT_COMMAND:+;$PROMPT_COMMAND}\""
        ));
        for (chain, original) in [
            (ZSH_ZSHENV, ".zshenv"),
            (ZSH_ZPROFILE, ".zprofile"),
            (ZSH_ZLOGIN, ".zlogin"),
            (ZSH_ZSHRC, ".zshrc"),
        ] {
            assert!(
                chain.contains(&format!("${{DROPTERM_ZDOTDIR:-$HOME}}/{original}")),
                "the {original} chain sources its original"
            );
        }
        assert!(ZSH_ZSHRC.contains("precmd_functions=(__dropterm_precmd $precmd_functions)"));
        assert!(ZSH_ZSHRC.contains("preexec_functions=(__dropterm_preexec $preexec_functions)"));
    }

    #[test]
    fn the_cwd_report_escapes_the_bytes_a_url_must_not_carry() {
        // The consumer percent-decodes (`ReportedCwd::parse`), so a
        // literal `%` or space in the path would mis-decode or
        // mangle the URL: every shell escapes both before the
        // report, and the decode round-trips.
        assert!(BASH_RCFILE.contains("${PWD//[%]/%25}"));
        assert!(BASH_RCFILE.contains("${__dropterm_path//[ ]/%20}"));
        assert!(ZSH_ZSHRC.contains("${PWD//[%]/%25}"));
        assert!(ZSH_ZSHRC.contains("${__dropterm_path//[ ]/%20}"));
        assert!(FISH_EMITTER.contains("string replace --all -- '%' '%25'"));
        assert!(FISH_EMITTER.contains("string replace --all -- ' ' '%20'"));
    }
}
