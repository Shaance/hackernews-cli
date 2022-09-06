mod lib;

use std::collections::HashSet;

use crate::lib::{HackerNewsClient, HackerNewsClientImpl};
use anyhow::Result;
use clap::Parser;

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

fn get_valid_story_types() -> HashSet<&'static str> {
    HashSet::from(["best", "new", "top"])
}

async fn fetch_top_n_stories(
    client: &impl HackerNewsClient,
    story_type: &str,
    n: u8,
) -> Result<Vec<lib::HNCLIItem>, Box<dyn std::error::Error>> {
    let ids = client.get_stories(story_type).await?;
    // fetches a lot of ids by default, limit that by length given in args
    let ids = &ids[..n as usize];
    Ok(client.get_items(ids).await?)
}

fn validate_args(args: &Cli) -> Result<(), anyhow::Error> {
    if !get_valid_story_types().contains(&args.story_type.as_str()) {
        return Err(anyhow::anyhow!("Invalid story type: {}", args.story_type));
    }
    Ok(())
}

async fn run(args: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let hn_client: HackerNewsClientImpl = HackerNewsClient::new();
    let items = fetch_top_n_stories(&hn_client, &args.story_type, args.length).await?;
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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();
    let valid_args = validate_args(&args);
    if valid_args.is_err() {
        eprintln!("Error: {}", valid_args.err().unwrap());
        std::process::exit(exitcode::USAGE);
    }
    let result = run(args).await;
    match result {
        Ok(_) => {
            println!("Done!");
            std::process::exit(exitcode::OK);
        }
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
        let valid_story_types = get_valid_story_types();
        for story_type in ["best", "new", "top", "not_ok", "invalid", "etc"].iter() {
            let args = Cli {
                story_type: story_type.to_string(),
                length: 35, // length is validated by clap
            };
            let result = validate_args(&args);
            if valid_story_types.contains(story_type) {
                assert!(result.is_ok());
            } else {
                assert!(result.is_err());
            }
        }
    }
}
