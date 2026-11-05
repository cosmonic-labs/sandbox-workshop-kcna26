//! gh-not-installed: the placeholder behind the gh slot until step 4.
//!
//! Like not-installed, it answers the tool's interface with "isn't installed
//! yet". It also answers GitHub sign-in, which the agent harness imports
//! unlabelled: until a workload declares the github-api plugin (step 4),
//! something in the workload has to export it, or the harness could not
//! start. Only one component may, so this is a separate image from
//! not-installed, which sits behind several slots at once.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/gh-not-installed",
    generate_all,
});

use exports::workshop::toolbox::github_auth::{self, DevicePrompt, SignInStatus};
use exports::workshop::toolbox::gh;

struct NotInstalled;

impl gh::Guest for NotInstalled {
    async fn run(_: gh::RunArgs) -> Result<String, gh::Error> {
        Err(gh::Error::Other(Some("the gh tool isn't installed yet".into())))
    }
}

impl github_auth::Guest for NotInstalled {
    async fn start() -> Result<DevicePrompt, String> {
        Err("GitHub sign-in isn't part of this step: step 4's manifest declares the \
             github-auth capability that provides it"
            .into())
    }

    async fn status() -> Result<SignInStatus, String> {
        Ok(SignInStatus::SignedOut)
    }

    async fn sign_out() -> Result<(), String> {
        Ok(())
    }
}

export!(NotInstalled);
