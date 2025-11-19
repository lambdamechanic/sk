use crate::{git, lock};
use anyhow::{bail, Result};
use clap::Command;
use clap_complete::Shell;
use std::env;
use std::io::{self, Write};

pub fn generate(shell_name: &str, cmd: &mut Command) -> Result<()> {
    let shell = match shell_name {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        "elvish" => Shell::Elvish,
        "powershell" => Shell::PowerShell,
        other => bail!("Unsupported shell: {other}"),
    };

    let mut buffer = Vec::new();
    clap_complete::generate(shell, cmd, "sk", &mut buffer);
    let script = String::from_utf8(buffer)?;
    let script = match shell {
        Shell::Bash => inject_bash(script),
        Shell::Zsh => inject_zsh(script),
        Shell::Fish => inject_fish(script),
        _ => script,
    };
    io::stdout().write_all(script.as_bytes())?;
    Ok(())
}

pub fn emit_skill_names() -> Result<()> {
    let project_root = match git::ensure_git_repo() {
        Ok(root) => root,
        Err(err) => {
            log_completion_error(&err);
            return Ok(());
        }
    };
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        return Ok(());
    }
    let lockfile = match lock::Lockfile::load(&lock_path) {
        Ok(lf) => lf,
        Err(err) => {
            log_completion_error(&err);
            return Ok(());
        }
    };
    let mut names: Vec<String> = lockfile
        .skills
        .iter()
        .map(|skill| skill.install_name.clone())
        .collect();
    names.sort();
    names.dedup();
    for name in names {
        println!("{name}");
    }
    Ok(())
}

fn inject_bash(mut script: String) -> String {
    const CASE_ANCHOR: &str = "    case \"${cmd}\" in";
    if let Some(idx) = script.find(CASE_ANCHOR) {
        let hook = "    if __sk_dynamic_complete \"${cmd}\" \"${cur}\" \"${prev}\"; then\n        return 0\n    fi\n\n";
        script.insert_str(idx, hook);
    }
    script = script.replacen(
        "opts=\"-h --root --dry-run --help <TARGET>\"",
        "opts=\"-h --root --dry-run --help --all <TARGET>\"",
        1,
    );
    const REGISTRATION: &str = "\nif [[ \"${BASH_VERSINFO[0]}\"";
    if let Some(idx) = script.find(REGISTRATION) {
        script.insert_str(idx, BASH_HELPERS);
    } else {
        script.push_str(BASH_HELPERS);
    }
    script
}

fn inject_zsh(mut script: String) -> String {
    script = script.replace(
        ":installed_name:_default",
        ":installed_name:_sk_skill_names",
    );
    script = script.replace("*::names:_default", "*::names:_sk_skill_names");
    script = script.replace(":target:_default", ":target:_sk_upgrade_targets");
    script = script.replacen(
        "'--dry-run[]' \\",
        "'--dry-run[]' \\\n'--all[Upgrade every installed skill]' \\",
        1,
    );
    script.push_str(ZSH_HELPERS);
    script
}

fn inject_fish(mut script: String) -> String {
    script.push_str(FISH_HELPERS);
    script
}

fn log_completion_error(err: &dyn std::fmt::Display) {
    if env::var_os("SK_COMPLETIONS_DEBUG").is_some() {
        eprintln!("sk completions dynamic hook: {err}");
    }
}

const BASH_HELPERS: &str = r#"
__sk_dynamic_complete() {
    local cmd="$1"
    local cur="$2"
    local prev="$3"

    case "$cmd" in
        sk__where)
            if [[ "$cur" == -* || "$prev" == "--root" ]]; then
                return 1
            fi
            __sk_comp_reply_from_list "$(__sk_fetch_skill_names)" "$cur"
            return $?
            ;;
        sk__remove)
            if [[ "$cur" == -* || "$prev" == "--root" ]]; then
                return 1
            fi
            __sk_comp_reply_from_list "$(__sk_fetch_skill_names)" "$cur"
            return $?
            ;;
        sk__sync__back)
            if [[ "$cur" == -* ]]; then
                return 1
            fi
            case "$prev" in
                --root|--branch|--message|--repo|--skill-path)
                    return 1
                    ;;
            esac
            __sk_comp_reply_from_list "$(__sk_fetch_skill_names)" "$cur"
            return $?
            ;;
        sk__check|sk__status|sk__diff|sk__doctor)
            if [[ "$cur" == -* ]]; then
                return 1
            fi
            __sk_comp_reply_from_list "$(__sk_fetch_skill_names)" "$cur"
            return $?
            ;;
        sk__upgrade)
            if [[ "$prev" == "--root" ]]; then
                return 1
            fi
            if [[ "$cur" == -* && "$cur" != --all* ]]; then
                return 1
            fi
            __sk_comp_reply_from_list "$(__sk_upgrade_completion_targets)" "$cur"
            return $?
            ;;
        *)
            return 1
            ;;
    esac
}

__sk_comp_reply_from_list() {
    local list="$1"
    local cur="$2"
    if [[ -z "$list" ]]; then
        return 1
    fi
    COMPREPLY=( $(compgen -W "${list}" -- "${cur}") )
    return 0
}

__sk_upgrade_completion_targets() {
    printf '%s\n' --all
    __sk_fetch_skill_names
}

__sk_fetch_skill_names() {
    local bin="${COMP_WORDS[0]}"
    if [[ -z "$bin" ]]; then
        return
    fi
    command "$bin" completions --skills 2>/dev/null
}
"#;

const ZSH_HELPERS: &str = r#"
_sk_fetch_skill_names() {
    local bin=${words[1]}
    if [[ -z "$bin" ]]; then
        return
    fi
    command "$bin" completions --skills 2>/dev/null
}

_sk_skill_names() {
    local -a skill_names
    skill_names=(${(f)$(_sk_fetch_skill_names)})
    _describe 'installed skill' skill_names "$@"
}

_sk_upgrade_targets() {
    local -a targets
    targets=(--all ${(@f)$(_sk_fetch_skill_names)})
    _describe 'upgrade target' targets "$@"
}
"#;

const FISH_HELPERS: &str = r#"
function __fish_sk_skill_names
    set -l parts (commandline -opc)
    if test (count $parts) -eq 0
        return
    end
    command $parts[1] completions --skills 2>/dev/null
end

function __fish_sk_upgrade_targets
    printf -- "--all\n"
    __fish_sk_skill_names
end

complete -c sk -n "__fish_sk_using_subcommand where" -a "(__fish_sk_skill_names)"
complete -c sk -n "__fish_sk_using_subcommand remove" -a "(__fish_sk_skill_names)"
complete -c sk -n "__fish_sk_using_subcommand sync-back" -a "(__fish_sk_skill_names)"
complete -c sk -n "__fish_sk_using_subcommand check" -a "(__fish_sk_skill_names)"
complete -c sk -n "__fish_sk_using_subcommand status" -a "(__fish_sk_skill_names)"
complete -c sk -n "__fish_sk_using_subcommand diff" -a "(__fish_sk_skill_names)"
complete -c sk -n "__fish_sk_using_subcommand doctor" -a "(__fish_sk_skill_names)"
complete -c sk -n "__fish_sk_using_subcommand upgrade" -a "(__fish_sk_upgrade_targets)"
"#;
