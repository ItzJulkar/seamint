use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

const VALID_KEY: &str = "0x2222222222222222222222222222222222222222222222222222222222222222";
const VALID_ADDRESS: &str = "0x1563915e194D8CfBA1943570603F7606A3115508";
const RPC_URL: &str = "https://rpc.mainnet.chain.robinhood.com";

fn write_env(dir: &std::path::Path, body: &str) {
    std::fs::write(dir.join(".env"), body).expect("write .env");
}

fn minimal_env(key: Option<&str>) -> String {
    let key_line = key.map(|k| format!("WALLET_KEY={k}\n")).unwrap_or_default();
    format!(
        "RPC_URL={}\nFEE_AUTOMATIC=true\nGAS_LIMIT=300000\n{key_line}",
        RPC_URL
    )
}

#[test]
fn wallet_help_exposes_show_and_set() {
    Command::cargo_bin("seamint")
        .expect("binary")
        .args(["wallet", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("show"))
        .stdout(predicate::str::contains("set"));
}

#[test]
fn wallet_show_prints_configured_signing_address() {
    let dir = tempdir().expect("tempdir");
    write_env(dir.path(), &minimal_env(Some(VALID_KEY)));
    Command::cargo_bin("seamint")
        .expect("binary")
        .current_dir(dir.path())
        .args(["wallet", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains(VALID_ADDRESS))
        .stdout(predicate::str::contains("single-wallet"));
}

#[test]
fn wallet_show_warns_when_no_wallet_is_configured() {
    let dir = tempdir().expect("tempdir");
    write_env(dir.path(), &minimal_env(None));
    Command::cargo_bin("seamint")
        .expect("binary")
        .current_dir(dir.path())
        .args(["wallet", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No wallet is configured"));
}

#[test]
fn wallet_show_describes_multi_wallet_manifest_mode() {
    let dir = tempdir().expect("tempdir");
    write_env(
        dir.path(),
        &format!(
            "RPC_URL={}\nFEE_AUTOMATIC=true\nGAS_LIMIT=300000\nWALLETS_FILE=wallets.json\nSPONSORED=true\n",
            RPC_URL
        ),
    );
    std::fs::write(
        dir.path().join("wallets.json"),
        format!(
            "{{\"version\":1,\"wallets\":[{{\"private_key\":\"{VALID_KEY}\",\"quantity\":1}}]}}"
        ),
    )
    .expect("write manifest");
    Command::cargo_bin("seamint")
        .expect("binary")
        .current_dir(dir.path())
        .args(["wallet", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("sponsored"))
        .stdout(predicate::str::contains(VALID_ADDRESS));
}

#[test]
fn wallet_set_rejects_an_invalid_private_key() {
    let dir = tempdir().expect("tempdir");
    write_env(dir.path(), &minimal_env(Some(VALID_KEY)));
    Command::cargo_bin("seamint")
        .expect("binary")
        .current_dir(dir.path())
        .args(["wallet", "set", "--key", "0xnot-a-key"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("valid private key"));
}

#[test]
fn wallet_set_writes_key_and_preserves_other_settings() {
    let dir = tempdir().expect("tempdir");
    write_env(dir.path(), &minimal_env(None));
    Command::cargo_bin("seamint")
        .expect("binary")
        .current_dir(dir.path())
        .write_stdin("y\n")
        .args(["wallet", "set", "--key", VALID_KEY])
        .assert()
        .success()
        .stdout(predicate::str::contains(VALID_ADDRESS));
    let after = std::fs::read_to_string(dir.path().join(".env")).expect("read .env");
    assert!(
        after.contains(&format!("WALLET_KEY={VALID_KEY}")),
        "key written"
    );
    assert!(after.contains(RPC_URL), "unrelated setting preserved");
}

#[test]
fn wallet_set_is_cancelled_on_negative_answer() {
    let dir = tempdir().expect("tempdir");
    write_env(dir.path(), &minimal_env(None));
    Command::cargo_bin("seamint")
        .expect("binary")
        .current_dir(dir.path())
        .write_stdin("n\n")
        .args(["wallet", "set", "--key", VALID_KEY])
        .assert()
        .success(); // cancelled is treated as a clean exit
    let after = std::fs::read_to_string(dir.path().join(".env")).expect("read .env");
    assert!(!after.contains("WALLET_KEY="), "nothing written on cancel");
}

#[test]
fn wallet_set_refuses_while_manifest_is_active_without_sponsorship() {
    let dir = tempdir().expect("tempdir");
    write_env(
        dir.path(),
        &format!(
            "RPC_URL={}\nFEE_AUTOMATIC=true\nGAS_LIMIT=300000\nWALLETS_FILE=wallets.json\n",
            RPC_URL
        ),
    );
    Command::cargo_bin("seamint")
        .expect("binary")
        .current_dir(dir.path())
        .args(["wallet", "set", "--key", VALID_KEY])
        .assert()
        .failure()
        .stderr(predicate::str::contains("WALLETS_FILE"));
}

#[test]
fn wallet_set_refuses_an_empty_manifest_line_like_the_config_loader() {
    // An empty WALLETS_FILE= line is still "present" to the config loader, so
    // setting WALLET_KEY on top must be refused (it would make loading fail).
    let dir = tempdir().expect("tempdir");
    write_env(
        dir.path(),
        &format!(
            "RPC_URL={}\nFEE_AUTOMATIC=true\nGAS_LIMIT=300000\nWALLETS_FILE=\n",
            RPC_URL
        ),
    );
    Command::cargo_bin("seamint")
        .expect("binary")
        .current_dir(dir.path())
        .args(["wallet", "set", "--key", VALID_KEY])
        .assert()
        .failure()
        .stderr(predicate::str::contains("WALLETS_FILE"));
}

#[test]
fn wallet_show_reports_an_empty_manifest_line_as_conflict_not_missing() {
    let dir = tempdir().expect("tempdir");
    write_env(
        dir.path(),
        &format!(
            "RPC_URL={}\nFEE_AUTOMATIC=true\nGAS_LIMIT=300000\nWALLETS_FILE=\nWALLET_KEY={VALID_KEY}\n",
            RPC_URL
        ),
    );
    Command::cargo_bin("seamint")
        .expect("binary")
        .current_dir(dir.path())
        .args(["wallet", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Both WALLET_KEY and WALLETS_FILE are set",
        ))
        .stdout(predicate::str::contains("No wallet is configured").not());
}

#[test]
fn wallet_set_overwrites_an_existing_key_line() {
    let dir = tempdir().expect("tempdir");
    write_env(
        dir.path(),
        &format!(
            "RPC_URL={}\nFEE_AUTOMATIC=true\nGAS_LIMIT=300000\nWALLET_KEY=0x1111111111111111111111111111111111111111111111111111111111111111\n",
            RPC_URL
        ),
    );
    Command::cargo_bin("seamint")
        .expect("binary")
        .current_dir(dir.path())
        .write_stdin("y\n")
        .args(["wallet", "set", "--key", VALID_KEY])
        .assert()
        .success();
    let after = std::fs::read_to_string(dir.path().join(".env")).expect("read .env");
    assert_eq!(
        after
            .lines()
            .filter(|line| line.starts_with("WALLET_KEY="))
            .count(),
        1,
        "exactly one WALLET_KEY line remains after overwrite"
    );
    assert!(
        after.contains(&format!("WALLET_KEY={VALID_KEY}")),
        "new key written"
    );
}
