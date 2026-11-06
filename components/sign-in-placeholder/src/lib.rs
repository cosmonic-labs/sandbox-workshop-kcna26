//! sign-in-placeholder: GitHub sign-in until step 4.
//!
//! The agent harness imports GitHub sign-in. Until a workload declares the
//! github-api plugin (step 4), something in the workload has to export it,
//! or the harness could not start. This answers that sign-in isn't part of
//! this step.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/sign-in-placeholder",
    generate_all,
});

use exports::workshop::toolbox::github_auth::{self, DevicePrompt, SignInStatus, WebError};

struct Placeholder;

/// Why sign-in answers no before step 4.
const NOT_THIS_STEP: &str = "GitHub sign-in isn't part of this step: step 4's manifest declares \
                             the github-auth capability that provides it";

impl github_auth::Guest for Placeholder {
    async fn start() -> Result<DevicePrompt, String> {
        Err(NOT_THIS_STEP.into())
    }

    async fn begin_web(_: String) -> Result<String, WebError> {
        Err(WebError::NotSetUp(NOT_THIS_STEP.into()))
    }

    async fn complete_web(_: String, _: String) -> Result<String, WebError> {
        Err(WebError::NotSetUp(NOT_THIS_STEP.into()))
    }

    async fn status() -> Result<SignInStatus, String> {
        Ok(SignInStatus::SignedOut)
    }

    async fn sign_out() -> Result<(), String> {
        Ok(())
    }
}

export!(Placeholder);
