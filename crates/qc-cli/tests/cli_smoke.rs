use assert_cmd::Command;

#[test]
fn help_runs() {
    Command::cargo_bin("quickcut-cli")
        .unwrap()
        .arg("--help")
        .assert()
        .success();
}
