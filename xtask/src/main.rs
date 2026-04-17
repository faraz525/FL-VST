const DEFAULT_PACKAGE: &str = "dark_bassline";

fn main() -> nih_plug_xtask::Result<()> {
    let args = default_bundle_package(std::env::args().skip(1).collect(), DEFAULT_PACKAGE);
    nih_plug_xtask::main_with_args("cargo xtask", args)
}

fn default_bundle_package(mut args: Vec<String>, package: &str) -> Vec<String> {
    let Some(command) = args.first() else {
        return args;
    };

    if matches!(command.as_str(), "bundle" | "bundle-universal") {
        let has_positional_package = args.get(1).is_some_and(|arg| !arg.starts_with('-'));
        let has_package_flag = args
            .iter()
            .skip(1)
            .any(|arg| arg == "-p" || arg == "--package" || arg.starts_with("--package="));

        if !has_positional_package && !has_package_flag {
            args.insert(1, package.to_string());
        }
    }

    args
}

#[cfg(test)]
mod tests {
    use super::default_bundle_package;

    #[test]
    fn bundle_defaults_to_dark_bassline_package() {
        let args =
            default_bundle_package(vec!["bundle".into(), "--release".into()], "dark_bassline");
        assert_eq!(args, vec!["bundle", "dark_bassline", "--release"]);
    }

    #[test]
    fn bundle_keeps_explicit_package() {
        let args = default_bundle_package(
            vec!["bundle".into(), "other_plugin".into(), "--release".into()],
            "dark_bassline",
        );
        assert_eq!(args, vec!["bundle", "other_plugin", "--release"]);
    }

    #[test]
    fn bundle_keeps_package_flags() {
        let args = default_bundle_package(
            vec![
                "bundle".into(),
                "-p".into(),
                "other_plugin".into(),
                "--release".into(),
            ],
            "dark_bassline",
        );
        assert_eq!(args, vec!["bundle", "-p", "other_plugin", "--release"]);
    }

    #[test]
    fn bundle_inserts_package_before_other_build_flags() {
        let args = default_bundle_package(
            vec![
                "bundle".into(),
                "--target".into(),
                "x86_64-apple-darwin".into(),
                "--release".into(),
            ],
            "dark_bassline",
        );
        assert_eq!(
            args,
            vec![
                "bundle",
                "dark_bassline",
                "--target",
                "x86_64-apple-darwin",
                "--release",
            ]
        );
    }
}
