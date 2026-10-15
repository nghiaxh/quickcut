use assert_cmd::Command;

#[test]
fn help_runs() {
    Command::cargo_bin("quickcut")
        .unwrap()
        .arg("--help")
        .assert()
        .success();
}
