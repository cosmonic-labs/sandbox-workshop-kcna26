//! An empty tool slot. The agent world declares one labelled import per tool,
//! and each label must be filled by a workload component of that name. Until a
//! step installs the real tool, the manifest fills the slot with this stub,
//! which describes itself with an empty name so the agent leaves it out.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/tool-provider",
    generate_all,
});

use exports::workshop::toolbox::tool::Guest;
use workshop::toolbox::types::ToolDef;

struct Stub;

impl Guest for Stub {
    fn describe() -> ToolDef {
        ToolDef {
            name: String::new(),
            description: "not installed yet".into(),
            parameters: "{}".into(),
        }
    }

    fn call(_arguments: String) -> Result<String, String> {
        Err("this tool is not installed yet".into())
    }
}

export!(Stub);
