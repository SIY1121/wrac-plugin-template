use std::env;
use std::path::{Path, PathBuf};

use crate::Result;
use crate::constants::{AU_BUNDLE_NAME, CLAP_BUNDLE_NAME, STANDALONE_NAME, VST3_BUNDLE_NAME};
use crate::profile::BuildProfile;
use crate::targets::Platform;

pub(crate) struct Context {
    pub(crate) root: PathBuf,
    pub(crate) platform: Platform,
    pub(crate) target_dir: PathBuf,
    pub(crate) wrapper_dir: PathBuf,
}

impl Context {
    pub(crate) fn new() -> Result<Self> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(1)
            .ok_or("failed to locate repository root")?
            .to_path_buf();
        let target_dir = env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("target"));
        let wrapper_dir = env::var_os("CLAP_WRAPPER_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("clap_wrapper_builder"));

        Ok(Self {
            root,
            platform: Platform::detect()?,
            target_dir,
            wrapper_dir,
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

    pub(crate) fn artifacts_dir(&self, profile: BuildProfile) -> PathBuf {
        self.wrac_dir()
            .join("artifacts")
            .join(profile.artifact_dir())
    }

    pub(crate) fn cmake_dir(&self, purpose: &str, profile: BuildProfile) -> PathBuf {
        // Keep wrapper build directories fixed and short. The old scripts used
        // hashed names to dodge Windows path limits, but stable paths make
        // downstream tooling and .vscode/launch.json deterministic.
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
        self.artifacts_dir(profile).join(CLAP_BUNDLE_NAME)
    }

    pub(crate) fn vst3_bundle(&self, profile: BuildProfile) -> PathBuf {
        self.artifacts_dir(profile).join(VST3_BUNDLE_NAME)
    }

    pub(crate) fn au_bundle(&self, profile: BuildProfile) -> PathBuf {
        self.artifacts_dir(profile).join(AU_BUNDLE_NAME)
    }

    pub(crate) fn standalone_artifact(&self, profile: BuildProfile) -> PathBuf {
        let filename = if self.platform == Platform::Macos {
            format!("{STANDALONE_NAME}.app")
        } else {
            format!("{STANDALONE_NAME}.exe")
        };
        self.standalone_dir(profile).join(filename)
    }

    pub(crate) fn dynamic_library(&self, profile: BuildProfile) -> PathBuf {
        self.cargo_profile_dir(profile)
            .join(self.platform.dynamic_library_name())
    }

    pub(crate) fn static_library(&self, profile: BuildProfile) -> PathBuf {
        self.cargo_profile_dir(profile)
            .join(self.platform.static_library_name())
    }
}
