extern crate hn_lib;

use std::collections::HashSet;

use anyhow::Result;
use clap::Parser;
use hn_lib::{HNCLIItem, HackerNewsClient, HackerNewsClientImpl};

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
) -> Result<Vec<HNCLIItem>> {
    let ids = client
        .get_stories(story_type)
        .await
        .unwrap_or_else(|_| panic!("Failed to get ids from story type {}", story_type));
    // fetches a lot of ids by default, limit that by length given in args
    let ids = &ids[..n as usize];
    client.get_items(ids).await
}

fn validate_args(args: &Cli) -> Result<()> {
    match get_valid_story_types().contains(&args.story_type.as_str()) {
        true => Ok(()),
        false => Err(anyhow::anyhow!("Invalid story type: {}", args.story_type)),
    }
}

async fn run(args: Cli) -> Result<()> {
    let hn_client: HackerNewsClientImpl = HackerNewsClientImpl::new();
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
async fn main() -> Result<()> {
    let args = Cli::parse();

    if let Err(e) = validate_args(&args) {
        eprintln!("Error: {}", e);
        std::process::exit(exitcode::USAGE);
    }

    match run(args).await {
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
    use hn_lib::MockHackerNewsClient;
    use mockall::predicate;
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

    #[tokio::test]
    async fn test_fetch_top_n_stories() {
        let mut hn_client = MockHackerNewsClient::new();
        hn_client
            .expect_get_stories()
            .with(predicate::eq("best"))
            .times(1)
            .returning(|_| Ok(vec![1]));

        hn_client.expect_get_items().times(1).returning(|_| {
            Ok(vec![HNCLIItem {
                title: "".to_string(),
                url: "".to_string(),
                author: "".to_string(),
                time: "".to_string(),
                time_ago: "".to_string(),
                score: 0,
                comments: None,
            }])
        });

        let items = fetch_top_n_stories(&hn_client, "best", 1).await;

        assert!(items.is_ok());
        assert_eq!(items.unwrap().len(), 1);
    }
}
