use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

pub(crate) fn copy_path(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        copy_dir(from, to)?;
    } else {
        fs::copy(from, to)?;
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let destination = to.join(entry.file_name());
        if source.is_dir() {
            copy_dir(&source, &destination)?;
        } else {
            fs::copy(&source, &destination)?;
        }
    }
    Ok(())
}

pub(crate) fn ensure_exists(path: &Path, description: &str) -> Result<()> {
    if path.exists() {
        Ok(())
    } else {
        Err(format!("{description} not found: {}", path.display()).into())
    }
}

pub(crate) fn run(command: &mut Command) -> Result<()> {
    // xtask は build orchestration なので、失敗時に実際の外部 command が見えることが重要。
    // shell を経由せず Command で実行しつつ、人間が再実行しやすい形だけを表示する。
    println!("$ {}", format_command(command));
    let status = command.status()?;
    if !status.success() {
        return Err(format!(
            "command failed with status {status}: {}",
            format_command(command)
        )
        .into());
    }
    Ok(())
}

fn format_command(command: &Command) -> String {
    let mut parts = Vec::new();
    parts.push(shell_display(command.get_program()));
    parts.extend(command.get_args().map(shell_display));
    parts.join(" ")
}

fn shell_display(value: &OsStr) -> String {
    let text = value.to_string_lossy();
    if text.contains(' ') {
        format!("\"{text}\"")
    } else {
        text.into_owned()
    }
}

pub(crate) fn remove_if_exists(path: &Path) -> Result<()> {
    // clean/install は何度実行しても同じ結果にしたい。
    // missing を正常系にすることで、途中失敗後の再実行や dry な環境を扱いやすくする。
    if !path.exists() {
        return Ok(());
    }

    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub(crate) fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".into())
}

pub(crate) fn local_app_data() -> Result<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "LOCALAPPDATA is not set".into())
}

pub(crate) fn common_program_files() -> Result<PathBuf> {
    env::var_os("CommonProgramFiles")
        .map(PathBuf::from)
        .ok_or_else(|| "CommonProgramFiles is not set".into())
}

pub(crate) fn env_value_or(name: &str, fallback: &str) -> String {
    env::var(name).unwrap_or_else(|_| fallback.to_owned())
}

pub(crate) fn on_off(value: bool) -> &'static str {
    if value { "ON" } else { "OFF" }
}
