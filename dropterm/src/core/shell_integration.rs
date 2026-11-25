//! Shell integration (shell-integration spec): `DropTerm`'s own
//! emitter scripts — the OSC 133 prompt/command marks and the OSC 7
//! cwd report, written from the public specs alone (the clean-room
//! discipline: no bytes from any project's shell-integration text).
//! The scripts are embedded here and materialized under the cache
//! seat at spawn time; the injection decision below is this
//! module's other half.

use crate::core::config::DropTermConfig;
use crate::core::spawn::{SpawnCommand, SpawnParts};
use std::collections::HashMap;
use std::path::Path;

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

/// The spawn's integration verdict — the one function the spawn seam
/// needs.
#[derive(Debug, PartialEq)]
pub enum Injection {
    /// The spawn's argv/env gain the integration; `parts` is the
    /// integrated variant.
    Injected(SpawnParts),
    /// Untouched, and why — the skip log line's vocabulary.
    Skipped(SkipReason),
}

/// Why a spawn stayed untouched (shell-integration spec): the skip's
/// one-word story, the log line's vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The config's kill switch is off: the whole path — seat,
    /// argv, env — never runs.
    KillSwitch,
    /// The program's basename is not bash, zsh, or fish — `sh` (the
    /// default fallback), `nu`, `pwsh`, any program: spawn truth,
    /// never a process-name heuristic.
    UnsupportedShell,
    /// The user's own setup already carries the choice this
    /// injection would make (a `--rcfile`, fish's init flag, a
    /// profile env's `ZDOTDIR`): never fight the user's file
    /// choice.
    UserFlagConflict,
    /// A `--cmd` spawn: a line carries its own arguments and never
    /// inherits — `DropTerm` does not decorate a user's explicit
    /// program.
    CommandSpawn,
    /// The seat could not be materialized (no cache dir resolves, a
    /// write failed): a missing emitter must never half-integrate.
    Unmaterialized,
}

/// The shells this build integrates, from the spawn's program word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    /// The basename discrimination — spawn truth, the pty program's
    /// own file name, never a running-process guess.
    fn of(program: &str) -> Option<Self> {
        match Path::new(program).file_name()?.to_str()? {
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }
}

/// Whether the user's own setup already carries the choice injection
/// would make — the never-fight rule's scan.
fn flag_conflict(shell: Shell, trailing: &[String], env: &HashMap<String, String>) -> bool {
    match shell {
        Shell::Bash => trailing
            .iter()
            .any(|arg| arg == "--rcfile" || arg.starts_with("--rcfile=")),
        Shell::Fish => trailing.iter().any(|arg| {
            arg == "-C" || arg == "--init-command" || arg.starts_with("--init-command=")
        }),
        Shell::Zsh => env.contains_key("ZDOTDIR"),
    }
}

/// The env a zsh integration adds: the bootstrap dir as ZDOTDIR, the
/// original (the process's ZDOTDIR, else HOME) preserved for the
/// chain to source from. A machine with neither keeps
/// `DROPTERM_ZDOTDIR` unset — the bootstrap's own `:-$HOME` fallback
/// then sources nothing, the honest degrade.
fn zsh_env(seat: &Path, env: &mut HashMap<String, String>) {
    env.insert("ZDOTDIR".to_owned(), seat.join("zsh").display().to_string());
    if let Ok(original) = std::env::var("ZDOTDIR").or_else(|_| std::env::var("HOME")) {
        env.insert("DROPTERM_ZDOTDIR".to_owned(), original);
    }
}

/// The plain-spawn composition: bash gains the rcfile wrapper at the
/// head of the trailing arguments (the config's own args standing
/// behind it), zsh gains the bootstrap env with the argv untouched,
/// fish gains the init flag sourcing the emitter.
fn integrate_plain(shell: Shell, parts: &SpawnParts, seat: &Path) -> SpawnParts {
    let mut integrated = SpawnParts {
        program: parts.program.clone(),
        trailing: parts.trailing.clone(),
        profile_id: parts.profile_id.clone(),
        env: parts.env.clone(),
    };
    match shell {
        Shell::Bash => {
            let mut trailing = vec![
                "--rcfile".to_owned(),
                seat.join("dropterm.bash").display().to_string(),
            ];
            trailing.append(&mut integrated.trailing);
            integrated.trailing = trailing;
        }
        Shell::Zsh => zsh_env(seat, &mut integrated.env),
        Shell::Fish => {
            let mut trailing = vec![
                "-C".to_owned(),
                format!("source {}", seat.join("dropterm.fish").display()),
            ];
            trailing.append(&mut integrated.trailing);
            integrated.trailing = trailing;
        }
    }
    integrated
}

/// The profile-tail composition: the command-then-shell wrap's exec
/// line becomes the integrated spawn — the rcfile wrapper / the fish
/// flag on the exec'd shell itself, the ZDOTDIR env riding for the
/// tail alone to read — so the preamble phase emits nothing and the
/// status timer never sees a preamble's verdict.
fn integrate_tail(shell: Shell, parts: &SpawnParts, seat: &Path) -> Option<SpawnParts> {
    let mut integrated = SpawnParts {
        program: parts.program.clone(),
        trailing: parts.trailing.clone(),
        profile_id: parts.profile_id.clone(),
        env: parts.env.clone(),
    };
    let suffix = format!("; exec {}", parts.program);
    let line = integrated.trailing.get(1)?;
    let prefix = line.strip_suffix(&suffix)?;
    let exec = match shell {
        Shell::Bash => {
            format!(
                "; exec {} --rcfile {}",
                parts.program,
                seat.join("dropterm.bash").display()
            )
        }
        Shell::Fish => {
            format!(
                "; exec {} -C 'source {}'",
                parts.program,
                seat.join("dropterm.fish").display()
            )
        }
        Shell::Zsh => {
            zsh_env(seat, &mut integrated.env);
            suffix
        }
    };
    integrated.trailing[1] = format!("{prefix}{exec}");
    Some(integrated)
}

/// The decision, seat-resolved: the pure table the pins exercise.
/// The kill switch is `decide`'s own first gate, so this table
/// assumes the feature is on and the seat stands.
fn decide_against_seat(command: &SpawnCommand, parts: &SpawnParts, seat: &Path) -> Injection {
    // The `--cmd` rule reads across whole: never decorate a user's
    // explicit program, whatever it resolves to.
    if matches!(command, SpawnCommand::CommandLine(_)) {
        return Injection::Skipped(SkipReason::CommandSpawn);
    }
    let Some(shell) = Shell::of(&parts.program) else {
        return Injection::Skipped(SkipReason::UnsupportedShell);
    };
    if flag_conflict(shell, &parts.trailing, &parts.env) {
        log::warn!(
            "shell integration suppressed: the spawn's own arguments \
             already carry its file choice"
        );
        return Injection::Skipped(SkipReason::UserFlagConflict);
    }
    // A profile spawn whose wrap stands (the id records it)
    // integrates the interactive tail only; every degraded arm —
    // the blank line, the unrunnable shell — lands on the plain
    // shape and integrates as one.
    let integrated = if parts.profile_id.is_some() {
        integrate_tail(shell, parts, seat)
    } else {
        Some(integrate_plain(shell, parts, seat))
    };
    match integrated {
        Some(parts) => Injection::Injected(parts),
        // The wrap line did not end the way `spawn_parts` builds it
        // — unreachable without a resolver change; untouched is the
        // safe answer.
        None => Injection::Skipped(SkipReason::UnsupportedShell),
    }
}

/// The spawn's integration verdict (shell-integration spec): whether
/// this spawn's argv/env gain the emitters. The kill switch gates
/// whole — no materialization, no argv/env change, byte-identical to
/// today — then the seat materializes, then the pure table decides.
pub fn decide(command: &SpawnCommand, parts: &SpawnParts, config: &DropTermConfig) -> Injection {
    if !config.shell_integration {
        return Injection::Skipped(SkipReason::KillSwitch);
    }
    let Some(seat) = materialize() else {
        return Injection::Skipped(SkipReason::Unmaterialized);
    };
    decide_against_seat(command, parts, &seat)
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

    // --- the injection decision (SI-6) ---

    use crate::core::config::DropTermConfig;
    use crate::core::spawn::{SpawnCommand, SpawnParts};
    use std::collections::HashMap;

    fn parts(program: &str, trailing: &[&str]) -> SpawnParts {
        SpawnParts {
            program: program.to_owned(),
            trailing: trailing.iter().map(|s| (*s).to_owned()).collect(),
            profile_id: None,
            env: HashMap::new(),
        }
    }

    fn profile_parts(program: &str, command: &str) -> SpawnParts {
        SpawnParts {
            trailing: vec!["-c".to_owned(), format!("{command}; exec {program}")],
            profile_id: Some("named".to_owned()),
            ..parts(program, &[])
        }
    }

    fn shell_command(shell: &str) -> SpawnCommand {
        SpawnCommand::Shell(shell.to_owned())
    }

    /// The original ZDOTDIR the spawn preserves — the same
    /// resolution the decision reads (the process's ZDOTDIR, else
    /// HOME), mirrored so the pin and the code cannot drift.
    fn preserved_zdotdir() -> String {
        std::env::var("ZDOTDIR")
            .or_else(|_| std::env::var("HOME"))
            .expect("a ZDOTDIR or HOME in the test environment")
    }

    #[test]
    fn bash_gains_the_rcfile_wrapper_with_the_config_args_preserved() {
        // The wrapper rides at the head of the trailing arguments,
        // the config's own args standing behind it — `--login`
        // composes.
        let seat = seat_dir("decide-bash");
        let verdict = decide_against_seat(
            &shell_command("/bin/bash"),
            &parts("/bin/bash", &["--login"]),
            &seat,
        );
        match verdict {
            Injection::Injected(parts) => {
                assert_eq!(parts.program, "/bin/bash");
                assert_eq!(
                    parts.trailing,
                    vec![
                        "--rcfile".to_owned(),
                        seat.join("dropterm.bash").display().to_string(),
                        "--login".to_owned(),
                    ]
                );
            }
            Injection::Skipped(_) => panic!("a bash spawn integrates"),
        }
        let _ = std::fs::remove_dir_all(&seat);
    }

    #[test]
    fn zsh_gains_the_zdotdir_bootstrap_with_the_original_preserved() {
        // The argv stands untouched; the env carries the bootstrap
        // ZDOTDIR and the original the chain sources from.
        let seat = seat_dir("decide-zsh");
        let verdict = decide_against_seat(&shell_command("zsh"), &parts("zsh", &[]), &seat);
        match verdict {
            Injection::Injected(parts) => {
                assert_eq!(parts.trailing, Vec::<String>::new());
                assert_eq!(
                    parts.env.get("ZDOTDIR").map(String::as_str),
                    Some(seat.join("zsh").display().to_string().as_str())
                );
                assert_eq!(
                    parts.env.get("DROPTERM_ZDOTDIR").map(String::as_str),
                    Some(preserved_zdotdir().as_str())
                );
            }
            Injection::Skipped(_) => panic!("a zsh spawn integrates"),
        }
        let _ = std::fs::remove_dir_all(&seat);
    }

    #[test]
    fn fish_gains_the_init_flag_with_the_config_args_preserved() {
        // fish's own `-C` init command sources the emitter after
        // fish's config has run; the config's own args stand behind
        // the flag.
        let seat = seat_dir("decide-fish");
        let verdict = decide_against_seat(
            &shell_command("/usr/bin/fish"),
            &parts("/usr/bin/fish", &["--private"]),
            &seat,
        );
        match verdict {
            Injection::Injected(parts) => {
                assert_eq!(
                    parts.trailing,
                    vec![
                        "-C".to_owned(),
                        format!("source {}", seat.join("dropterm.fish").display()),
                        "--private".to_owned(),
                    ]
                );
            }
            Injection::Skipped(_) => panic!("a fish spawn integrates"),
        }
        let _ = std::fs::remove_dir_all(&seat);
    }

    #[test]
    fn unsupported_shells_and_programs_spawn_untouched() {
        // Spawn truth, never a heuristic: sh (the default fallback),
        // nu, pwsh, and any program a `--cmd` resolves to stay
        // byte-for-byte — the same parts back, the skip naming why.
        let seat = seat_dir("decide-unsupported");
        for program in ["/bin/sh", "sh", "nu", "pwsh", "htop"] {
            let original = parts(program, &["--login"]);
            match decide_against_seat(&shell_command(program), &original, &seat) {
                Injection::Skipped(reason) => {
                    assert_eq!(reason, SkipReason::UnsupportedShell, "{program}");
                }
                Injection::Injected(_) => panic!("{program} must not integrate"),
            }
        }
        let _ = std::fs::remove_dir_all(&seat);
    }

    #[test]
    fn a_cmd_spawn_never_integrates_whatever_it_resolves_to() {
        // The CommandLine arm's own rule reads across: a line
        // carries its own arguments and never inherits — not even a
        // `--cmd bash` gets decorated.
        let seat = seat_dir("decide-cmd");
        for line in ["htop", "bash"] {
            let original = parts("/bin/bash", &[]);
            match decide_against_seat(
                &SpawnCommand::CommandLine(line.to_owned()),
                &original,
                &seat,
            ) {
                Injection::Skipped(reason) => {
                    assert_eq!(reason, SkipReason::CommandSpawn, "{line}");
                }
                Injection::Injected(_) => panic!("--cmd {line} must not integrate"),
            }
        }
        let _ = std::fs::remove_dir_all(&seat);
    }

    #[test]
    fn a_user_flag_conflict_suppresses_the_injection() {
        // Never fight the user's own file choice: their `--rcfile`,
        // their fish init flag, their profile env's ZDOTDIR — the
        // spawn stays byte-for-byte.
        let seat = seat_dir("decide-conflict");
        let bash_conflict = parts("/bin/bash", &["--rcfile", "/my/own/rc"]);
        match decide_against_seat(&shell_command("/bin/bash"), &bash_conflict, &seat) {
            Injection::Skipped(reason) => assert_eq!(reason, SkipReason::UserFlagConflict),
            Injection::Injected(_) => panic!("a user --rcfile must suppress"),
        }
        let fish_conflict = parts("fish", &["--init-command=source /my/own.fish"]);
        match decide_against_seat(&shell_command("fish"), &fish_conflict, &seat) {
            Injection::Skipped(reason) => assert_eq!(reason, SkipReason::UserFlagConflict),
            Injection::Injected(_) => panic!("a user fish init flag must suppress"),
        }
        let mut zsh_env = HashMap::new();
        zsh_env.insert("ZDOTDIR".to_owned(), "/my/own/zdotdir".to_owned());
        let zsh_conflict = SpawnParts {
            env: zsh_env,
            ..parts("zsh", &[])
        };
        match decide_against_seat(&shell_command("zsh"), &zsh_conflict, &seat) {
            Injection::Skipped(reason) => assert_eq!(reason, SkipReason::UserFlagConflict),
            Injection::Injected(_) => panic!("a user ZDOTDIR must suppress"),
        }
        let _ = std::fs::remove_dir_all(&seat);
    }

    #[test]
    fn a_profile_spawn_integrates_the_interactive_tail_only() {
        // The command-then-shell wrap's exec line becomes the
        // integrated spawn; the preamble phase emits nothing — no
        // rcfile flag on the outer shell, the ZDOTDIR env riding
        // for the tail alone to read, the fish flag on the exec
        // line.
        let seat = seat_dir("decide-profile");
        let profile = SpawnCommand::Profile {
            name: "watch".to_owned(),
            command: "htop".to_owned(),
            cwd: None,
            env: None,
        };

        match decide_against_seat(&profile, &profile_parts("/bin/bash", "htop"), &seat) {
            Injection::Injected(parts) => {
                assert_eq!(parts.trailing[0], "-c");
                assert_eq!(
                    parts.trailing[1],
                    format!(
                        "htop; exec /bin/bash --rcfile {}",
                        seat.join("dropterm.bash").display()
                    )
                );
            }
            Injection::Skipped(_) => panic!("a bash-tailed profile integrates"),
        }

        match decide_against_seat(&profile, &profile_parts("zsh", "htop"), &seat) {
            Injection::Injected(parts) => {
                // The wrap line stands untouched; the env carries
                // the bootstrap for the exec'd tail.
                assert_eq!(parts.trailing[1], "htop; exec zsh");
                assert_eq!(
                    parts.env.get("ZDOTDIR").map(String::as_str),
                    Some(seat.join("zsh").display().to_string().as_str())
                );
            }
            Injection::Skipped(_) => panic!("a zsh-tailed profile integrates"),
        }

        match decide_against_seat(&profile, &profile_parts("fish", "htop"), &seat) {
            Injection::Injected(parts) => {
                assert_eq!(
                    parts.trailing[1],
                    format!(
                        "htop; exec fish -C 'source {}'",
                        seat.join("dropterm.fish").display()
                    )
                );
            }
            Injection::Skipped(_) => panic!("a fish-tailed profile integrates"),
        }
        let _ = std::fs::remove_dir_all(&seat);
    }

    #[test]
    fn a_degraded_profile_line_integrates_as_a_plain_shell() {
        // The blank-line degradation lands on the plain shell with
        // no id — the wrap is gone, so the plain shape integrates
        // (basename rules, argv), never a tail rewrite.
        let seat = seat_dir("decide-degraded");
        match decide_against_seat(
            &SpawnCommand::Profile {
                name: "broken".to_owned(),
                command: "   ".to_owned(),
                cwd: None,
                env: None,
            },
            &parts("/bin/bash", &[]),
            &seat,
        ) {
            Injection::Injected(parts) => {
                assert_eq!(parts.trailing.len(), 2, "the plain rcfile shape");
                assert_eq!(parts.trailing[0], "--rcfile");
            }
            Injection::Skipped(_) => panic!("a degraded profile line still integrates its shell"),
        }
        let _ = std::fs::remove_dir_all(&seat);
    }

    #[test]
    fn the_kill_switch_skips_whole_before_any_seat_work() {
        // The one `decide` pin: the switch is consulted before
        // materialization, so an off config never writes a byte —
        // and answers Skipped(KillSwitch) with no seat to read.
        let config = DropTermConfig {
            shell_integration: false,
            ..DropTermConfig::default()
        };
        match decide(
            &shell_command("/bin/bash"),
            &parts("/bin/bash", &[]),
            &config,
        ) {
            Injection::Skipped(reason) => assert_eq!(reason, SkipReason::KillSwitch),
            Injection::Injected(_) => panic!("the kill switch skips whole"),
        }
    }
}
