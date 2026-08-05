use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use capability_core::CapabilityCatalog;

fn main() -> ExitCode {
    match run() {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<String, String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let [catalog_path, format] = arguments.as_slice() else {
        return Err(
            "usage: capability-schema-cli <capabilities.json> <validate|codex|typescript>"
                .to_string(),
        );
    };
    let catalog_path = PathBuf::from(catalog_path);
    let source = fs::read_to_string(&catalog_path)
        .map_err(|error| format!("failed to read {}: {error}", catalog_path.display()))?;
    let catalog = serde_json::from_str::<CapabilityCatalog>(&source)
        .map_err(|error| format!("failed to parse {}: {error}", catalog_path.display()))?;
    match format.as_str() {
        "validate" => {
            catalog.validate().map_err(|error| error.to_string())?;
            Ok(format!("{} is valid\n", catalog_path.display()))
        }
        "codex" => serde_json::to_string_pretty(
            &catalog
                .codex_dynamic_tools(true)
                .map_err(|error| error.to_string())?,
        )
        .map(|value| format!("{value}\n"))
        .map_err(|error| error.to_string()),
        "typescript" => catalog
            .typescript_declarations()
            .map_err(|error| error.to_string()),
        unknown => Err(format!("unknown output format {unknown:?}")),
    }
}
