use assert_cmd::prelude::*;
use datadriven::walk;
use std::collections::HashMap;
use std::process::{Command, Stdio};

#[test]
// cli_integration_test ensures that the CLI commands themselves are
// passed through correctly to the Vault core logic.
fn cli_integration_test() {
    // Make a new directory for the vault.
    let dir = tempdir::TempDir::new("vault").unwrap();
    // Maintain a vec of instance directories at a higher scope so that
    // they are all dropped at the end of the test.
    let mut instance_dirs = vec![];
    let mut instances: HashMap<String, String> = HashMap::new();
    walk("tests/testdata/cli/integration", |f| {
        f.run(|test_case| -> String {
            match test_case.directive.as_str() {
                "cmd" => {
                    // Construct a Command for the Vault binary.
                    let mut cmd = Command::cargo_bin("vault").unwrap();
                    // Split the input on " " to get the individual arguments. This
                    // won't work if the input has spaces within strings, but that's
                    // not something that we should worry about here.
                    let args = test_case.input.trim().split(" ");
                    // Swap out any temporary directories in the input arguments with
                    // the real path of the directory. We'll look for arguments of the form
                    // tempdir=name.
                    let args: Vec<&str> = args
                        .map(|s| {
                            if s.starts_with("tempdir=") {
                                let name = &s["tempdir=".len()..];
                                // Return the directory that maps to the target name.
                                &instances[name]
                            } else {
                                s
                            }
                        })
                        .collect();
                    // Point the vault command at the temporary directory that we made,
                    // using the args from the input. Spawn a child process.
                    let child = cmd
                        .env("VAULT_DIR", dir.path().to_str().unwrap())
                        .args(args)
                        .stdout(Stdio::piped())
                        .spawn()
                        .expect("Unable to start vault binary");
                    // Wait for the child to finish, and collect its output.
                    let res = child.wait_with_output();
                    match res {
                        // TODO (rohany): Is just looking at stdout enough?
                        Ok(o) => {
                            let s = String::from_utf8(o.stdout).unwrap();
                            let mut s = s.trim().to_string();
                            if s != "" {
                                // Datadriven has an annoying requirement to include a newline after
                                // non-empty result strings.
                                s.push_str("\n")
                            }
                            s
                        }
                        Err(e) => format!("{}", e),
                    }
                }
                "new-instance" => {
                    // Create a new instance directory.
                    let name = test_case.args["name"][0].clone();
                    let dir = tempdir::TempDir::new("experiment").unwrap();
                    let path = dir.path().to_str().unwrap().to_string();
                    instances.insert(name.into(), path);
                    instance_dirs.push(dir);
                    "".to_string()
                }
                s => panic!("Unhandled command: {}", s),
            }
        })
    })
}
