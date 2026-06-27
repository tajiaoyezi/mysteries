mod agent;
mod error;
mod permission;
mod provider;
mod tool;

use crate::agent::run_single_turn;
use crate::error::ProviderError;
use crate::provider::mock::MockProvider;
use crate::provider::{DeltaSink, FinishReason, ModelResponse};
use std::env;
use std::io::{self, Write};

#[tokio::main]
async fn main() -> Result<(), ProviderError> {
    let prompt = read_prompt()
        .map_err(|err| ProviderError::Transport(format!("failed to read prompt: {err}")))?;
    let provider = MockProvider::new(vec![ModelResponse {
        text: format!("mock response: {prompt}"),
        tool_calls: Vec::new(),
        finish_reason: FinishReason::Stop,
    }]);
    let sink = StdoutSink;

    let _ = run_single_turn(&provider, &prompt, &sink).await?;
    println!();

    Ok(())
}

fn read_prompt() -> io::Result<String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if !args.is_empty() {
        return Ok(args.join(" "));
    }

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    while input.ends_with('\n') || input.ends_with('\r') {
        input.pop();
    }

    Ok(input)
}

struct StdoutSink;

impl DeltaSink for StdoutSink {
    fn on_text(&self, text: &str) {
        print!("{text}");
        let _ = io::stdout().flush();
    }
}
