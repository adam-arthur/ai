use std::{env, path::PathBuf, process};

use harness::{Agent, StructuredAgentModel, WorkspaceTools};
use llm::{Client, ModelId};

const USAGE: &str = "Usage: harness ask [--model MODEL] [--workspace PATH] <goal>";

struct Arguments {
    model: ModelId,
    workspace: PathBuf,
    goal: String,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = parse_arguments(env::args().skip(1))?;
    let workspace_tools = WorkspaceTools::new(&arguments.workspace)?;
    let client = Client::from_env()?;
    let model = StructuredAgentModel::builder(client, arguments.model).build();
    let mut agent = Agent::builder(model).build();
    workspace_tools.register(agent.tools_mut())?;

    let run = agent.run(arguments.goal).await?;
    println!("{}", run.output);
    Ok(())
}

fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Arguments, String> {
    let mut arguments = arguments.into_iter();
    if arguments.next().as_deref() != Some("ask") {
        return Err(USAGE.into());
    }

    let mut model = ModelId::GEMMA_4_12B_Q4;
    let mut workspace = env::current_dir().map_err(|error| error.to_string())?;
    let mut goal = Vec::new();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--model" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| format!("missing value for --model\n{USAGE}"))?;
                model = value.parse().map_err(|error| format!("{error}\n{USAGE}"))?;
            },
            "--workspace" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| format!("missing value for --workspace\n{USAGE}"))?;
                workspace = PathBuf::from(value);
            },
            "--help" | "-h" => return Err(USAGE.into()),
            option if option.starts_with('-') => return Err(format!("unknown option `{option}`\n{USAGE}")),
            _ => {
                goal.push(argument);
                goal.extend(arguments);
                break;
            },
        }
    }
    if goal.is_empty() {
        return Err(format!("missing goal\n{USAGE}"));
    }

    Ok(Arguments {
        model,
        workspace,
        goal: goal.join(" "),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_ask_command() {
        let arguments = parse_arguments([
            "ask".into(),
            "--model".into(),
            "GEMMA_4_E2B_Q4".into(),
            "--workspace".into(),
            "fixture".into(),
            "explain".into(),
            "the code".into(),
        ])
        .unwrap();

        assert_eq!(arguments.model, ModelId::GEMMA_4_E2B_Q4);
        assert_eq!(arguments.workspace, PathBuf::from("fixture"));
        assert_eq!(arguments.goal, "explain the code");
    }
}
