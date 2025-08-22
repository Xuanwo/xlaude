#[cfg(test)]
mod dashboard_tests {
    use assert_cmd::Command;
    use predicates::prelude::*;

    #[test]
    fn test_dashboard_without_tmux() {
        // If tmux is not available, should show helpful error
        if !tmux_available() {
            let mut cmd = Command::cargo_bin("xlaude").unwrap();
            cmd.arg("dashboard");

            cmd.assert()
                .failure()
                .stderr(predicate::str::contains("tmux is not installed"));
        }
    }

    #[test]
    fn test_dashboard_help() {
        let mut cmd = Command::cargo_bin("xlaude").unwrap();
        cmd.arg("dashboard").arg("--help");

        cmd.assert()
            .success()
            .stdout(predicate::str::contains("Launch interactive dashboard"));
    }

    fn tmux_available() -> bool {
        std::process::Command::new("which")
            .arg("tmux")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}
