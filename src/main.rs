extern crate hn_lib;

use std::collections::HashSet;

use anyhow::Result;
use clap::Parser;

use hn_lib::{HackerNewsCliService, HackerNewsCliServiceImpl};

#[derive(Parser, Debug)]
#[clap(
    name = "HN CLI",
    version = "1.0",
    about = "A command line interface for Hacker News"
)]
struct Cli {
    #[clap(short, long, default_value = "best")]
    /// The type of stories to retrieve, can be 'top', 'new' or 'best'
    story_type: String,
    #[clap(short, long, default_value_t=10, value_parser = clap::value_parser!(u8).range(1..=50))]
    /// The number of stories to retrieve. Should be between 1 and 50 inclusive
    length: u8,
}

fn validate_args(args: &Cli, valid_story_types: HashSet<&'static str>) -> Result<()> {
    match valid_story_types.contains(&args.story_type.as_str()) {
        true => Ok(()),
        false => Err(anyhow::anyhow!("Invalid story type: {}", args.story_type)),
    }
}

async fn run(args: Cli, service: &impl HackerNewsCliService) -> Result<()> {
    let items = service
        .fetch_top_n_stories(&args.story_type, args.length)
        .await?;
    for (idx, item) in items.iter().enumerate() {
        println!("\n#{} {}", idx + 1, item);
    }
    print!(
        "\n^ Enjoy the top {} {} HN stories! ^\n",
        args.length, args.story_type
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    let hn_cli_service = HackerNewsCliServiceImpl::new(None);

    if let Err(e) = validate_args(&args, HackerNewsCliServiceImpl::get_valid_story_types()) {
        eprintln!("Error: {}", e);
        std::process::exit(exitcode::USAGE);
    }

    match run(args, &hn_cli_service).await {
        Ok(_) => std::process::exit(exitcode::OK),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(exitcode::SOFTWARE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_args() {
        let valid_story_types = HackerNewsCliServiceImpl::get_valid_story_types();
        for story_type in ["best", "new", "top", "not_ok", "invalid", "etc"].into_iter() {
            let args = Cli {
                story_type: story_type.to_string(),
                length: 35, // length is validated by clap
            };
            let result = validate_args(&args, valid_story_types.clone());
            if valid_story_types.contains(story_type) {
                assert!(result.is_ok());
            } else {
                assert!(result.is_err());
            }
        }
    }
}
