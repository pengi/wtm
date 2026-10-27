//! Shell integration: completion and the change-directory function.

use anyhow::{Result, bail};
use clap_complete::env::{Bash, EnvCompleter, Zsh};

use crate::args::Shell;

/// Environment variable that switches wtm into completion mode.
pub const COMPLETE_VAR: &str = "COMPLETE";

/// Code for `eval "$(wtm shell-init <shell>)"`.
pub fn init(shell: Shell, cd_command: &str) -> Result<()> {
    if cd_command.is_empty()
        || !cd_command
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("invalid function name '{cd_command}'");
    }
    let mut out = String::new();
    match shell {
        Shell::Bash => {
            out.push_str(&registration(shell)?);
            out.push_str(&BASH_CD.replace("WCD", cd_command));
        }
        Shell::Zsh => {
            // Completion needs compinit; the function works without it.
            out.push_str("if (( $+functions[compdef] )); then\n");
            out.push_str(&registration(shell)?);
            out.push_str(&ZSH_CD_COMPLETION.replace("WCD", cd_command));
            out.push_str("fi\n");
            out.push_str(&ZSH_CD.replace("WCD", cd_command));
        }
    }
    print!("{out}");
    Ok(())
}

/// A completion file to install system-wide: for bash-completion's
/// completions directory, or a zsh `_wtm` function file in `$fpath`.
pub fn completion_file(shell: Shell) -> Result<String> {
    let script = registration(shell)?;
    Ok(match shell {
        Shell::Bash => script,
        // An autoloaded function file is run on each completion, so it must
        // complete rather than register.
        Shell::Zsh => script.replace(
            "compdef _clap_dynamic_completer_wtm wtm",
            "_clap_dynamic_completer_wtm \"$@\"",
        ),
    })
}

fn registration(shell: Shell) -> Result<String> {
    let mut buf = Vec::new();
    let completer: &dyn EnvCompleter = match shell {
        Shell::Bash => &Bash,
        Shell::Zsh => &Zsh,
    };
    completer.write_registration(COMPLETE_VAR, "wtm", "wtm", "wtm", &mut buf)?;
    Ok(String::from_utf8(buf)?)
}

const BASH_CD: &str = r#"
WCD() {
    local dir
    dir="$(command wtm locate ${1:+"$1"})" && builtin cd -- "$dir"
}
_wtm_WCD() {
    local cur="${COMP_WORDS[COMP_CWORD]}"
    COMPREPLY=($(compgen -W "$(command wtm list --names 2>/dev/null)" -- "$cur"))
}
complete -F _wtm_WCD WCD
"#;

const ZSH_CD: &str = r#"
WCD() {
    local dir
    dir="$(command wtm locate ${1:+"$1"})" && builtin cd -- "$dir"
}
"#;

const ZSH_CD_COMPLETION: &str = r#"
_wtm_WCD() {
    local -a names
    names=(${(f)"$(command wtm list --names 2>/dev/null)"})
    compadd -a names
}
compdef _wtm_WCD WCD
"#;
