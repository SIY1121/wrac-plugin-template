use std::env;
use std::path::{Path, PathBuf};

use crate::Result;
use crate::metadata::PluginMetadata;
use crate::profile::BuildProfile;
use crate::targets::Platform;

pub(crate) struct Context {
    pub(crate) root: PathBuf,
    pub(crate) platform: Platform,
    pub(crate) target_dir: PathBuf,
    pub(crate) wrapper_dir: PathBuf,
    pub(crate) metadata: PluginMetadata,
}

impl Context {
    pub(crate) fn new() -> Result<Self> {
        // cargo xtask is invoked from the xtask crate's manifest, so the parent directory is the repo root.
        // Relying on current_dir would misalign artifact paths when invoked from a different directory.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(1)
            .ok_or("failed to locate repository root")?
            .to_path_buf();
        // CARGO_TARGET_DIR may be redirected to a shared cache in workspaces or CI.
        // Using the same target root as cargo keeps post-build library detection consistent.
        let target_dir = env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("target"));
        // The in-repo submodule is used by default to keep wrapper forks and patches minimal.
        // CLAP_WRAPPER_DIR is an escape hatch for testing SDK changes or a temporary external checkout.
        let wrapper_dir = env::var_os("CLAP_WRAPPER_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("clap_wrapper_builder"));
        // Plugin identity is sourced from [package.metadata.wrac] in src-plugin/Cargo.toml.
        // Maintaining separate bundle names or wrapper arguments in xtask risks stale build artifacts on rename.
        let metadata = PluginMetadata::read(&root.join("src-plugin").join("Cargo.toml"))?;

        Ok(Self {
            root,
            platform: Platform::detect()?,
            target_dir,
            wrapper_dir,
            metadata,
        })
    }

    pub(crate) fn gui_dir(&self) -> PathBuf {
        self.root.join("src-gui")
    }

    pub(crate) fn plugin_manifest(&self) -> PathBuf {
        self.root.join("src-plugin").join("Cargo.toml")
    }

    pub(crate) fn cargo_profile_dir(&self, profile: BuildProfile) -> PathBuf {
        self.target_dir.join(profile.cargo_dir())
    }

    pub(crate) fn wrac_dir(&self) -> PathBuf {
        self.target_dir.join("wrac")
    }

    pub(crate) fn plugins_dir(&self, profile: BuildProfile) -> PathBuf {
        self.wrac_dir().join("plugins").join(profile.artifact_dir())
    }

    pub(crate) fn cmake_dir(&self, purpose: &str, profile: BuildProfile) -> PathBuf {
        // Keep the wrapper build directory short and stable.
        // The old hash-based path helped avoid Windows path length limits but hurt reproducibility in launch.json and investigations.
        self.wrac_dir()
            .join("cmake")
            .join(format!("{purpose}-{}", profile.cmake_suffix()))
    }

    pub(crate) fn standalone_dir(&self, profile: BuildProfile) -> PathBuf {
        self.wrac_dir()
            .join("standalone")
            .join(profile.artifact_dir())
    }

    pub(crate) fn clap_bundle(&self, profile: BuildProfile) -> PathBuf {
        self.plugins_dir(profile)
            .join(self.metadata.clap_bundle_name())
    }

    pub(crate) fn vst3_bundle(&self, profile: BuildProfile) -> PathBuf {
        self.plugins_dir(profile)
            .join(self.metadata.vst3_bundle_name())
    }

    pub(crate) fn au_bundle(&self, profile: BuildProfile) -> PathBuf {
        self.plugins_dir(profile)
            .join(self.metadata.au_bundle_name())
    }

    pub(crate) fn standalone_artifact(&self, profile: BuildProfile) -> PathBuf {
        let filename = match self.platform {
            Platform::Macos => format!("{}.app", self.metadata.standalone_name),
            Platform::Windows => format!("{}.exe", self.metadata.standalone_name),
            Platform::Linux => self.metadata.standalone_name.clone(),
        };
        self.standalone_dir(profile).join(filename)
    }

    pub(crate) fn dynamic_library(&self, profile: BuildProfile) -> PathBuf {
        self.cargo_profile_dir(profile)
            .join(self.platform.dynamic_library_name())
    }
}
