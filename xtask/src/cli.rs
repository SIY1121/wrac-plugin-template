use clap::{Args, Parser, Subcommand};

use crate::targets::{PluginTarget, Target, ValidateTarget};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Build and validate WRAC plugin artifacts")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    /// Build all OS-supported targets by default.
    Build(BuildArgs),
    /// Install previously built artifacts to user-local plugin folders.
    Install(InstallArgs),
    /// Remove installed user-local plugin artifacts.
    Uninstall(UninstallArgs),
    /// Validate previously built VST3/AU artifacts where supported.
    Validate(ValidateArgs),
    /// Remove generated build artifacts managed by xtask.
    Clean,
}

#[derive(Debug, Args)]
pub(crate) struct BuildArgs {
    /// Build with the release profile.
    #[arg(long)]
    pub(crate) release: bool,

    /// Remove generated plugin artifacts before building.
    #[arg(long)]
    pub(crate) clean: bool,

    /// Targets to build, comma-separated. Defaults to all OS-supported targets.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    pub(crate) target: Vec<Target>,

    /// Install artifacts after a successful build.
    #[arg(long)]
    pub(crate) install: bool,

    /// Validate generated VST3/AU artifacts after a successful build.
    #[arg(long)]
    pub(crate) validate: bool,
}

#[derive(Debug, Args)]
pub(crate) struct InstallArgs {
    /// Install release artifacts.
    #[arg(long)]
    pub(crate) release: bool,

    /// Targets to install, comma-separated. Defaults to OS-supported plugin formats.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    pub(crate) target: Vec<PluginTarget>,
}

#[derive(Debug, Args)]
pub(crate) struct UninstallArgs {
    /// Targets to uninstall, comma-separated. Defaults to OS-supported plugin formats.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    pub(crate) target: Vec<PluginTarget>,

    /// Print paths that would be removed without deleting them.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ValidateArgs {
    /// Use release artifacts.
    #[arg(long)]
    pub(crate) release: bool,

    /// Targets to validate, comma-separated. Defaults to VST3/AU where supported.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    pub(crate) target: Vec<ValidateTarget>,
}
