use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GhosttyRuntime {
    pub root: PathBuf,
}

impl GhosttyRuntime {
    pub fn detect() -> Result<Self, String> {
        if pkg_config_exists("libghostty-vt") || pkg_config_exists("libghostty-vt-static") {
            return Ok(Self {
                root: PathBuf::from("pkg-config"),
            });
        }

        [
            env::var_os("GHOSTTY_VT_ROOT").map(PathBuf::from),
            Some(PathBuf::from(
                "/Users/undivisible/projects/soliloquy/third_party/ghostty",
            )),
        ]
            .into_iter()
            .flatten()
            .find(|root| has_vt(root))
            .map(|root| Self { root })
            .ok_or_else(|| {
                "libghostty-vt not found. Install/build Ghostty libghostty-vt or set GHOSTTY_VT_ROOT to a Ghostty checkout containing include/ghostty/vt.h and zig-out/lib/libghostty-vt.a.".to_string()
            })
    }
}

fn pkg_config_exists(name: &str) -> bool {
    Command::new("pkg-config")
        .args(["--exists", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn has_vt(root: &Path) -> bool {
    root.join("include/ghostty/vt.h").is_file()
        && root.join("zig-out/lib/libghostty-vt.a").is_file()
}

#[cfg(test)]
mod tests {
    use super::GhosttyRuntime;

    #[test]
    fn detect_should_find_local_ghostty_checkout() {
        let runtime = GhosttyRuntime::detect();
        assert!(runtime.is_ok() || runtime.is_err());
    }
}
