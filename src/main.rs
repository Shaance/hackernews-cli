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
    #[clap(short, long, default_value_t = 1)]
    /// Page number (starts from 1)
    page: u32,
}

fn validate_args(args: &Cli, valid_story_types: HashSet<&'static str>) -> Result<()> {
    if !valid_story_types.contains(&args.story_type.as_str()) {
        return Err(anyhow::anyhow!("Invalid story type: {}", args.story_type));
    }

    if args.page == 0 {
        return Err(anyhow::anyhow!("Page number must be greater than 0"));
    }

    Ok(())
}

async fn run(args: Cli, service: &impl HackerNewsCliService) -> Result<()> {
    let items = service
        .fetch_stories_page(&args.story_type, args.length, args.page)
        .await?;

    if items.is_empty() {
        println!("No more stories available on page {}", args.page);
        return Ok(());
    }

    for (idx, item) in items.iter().enumerate() {
        let global_idx = ((args.page - 1) as usize * args.length as usize) + idx + 1;
        println!("\n#{} {}", global_idx, item);
    }

    print!(
        "\n^ Page {} of {} {} stories (showing {} items) ^\n",
        args.page,
        args.story_type,
        items.len(),
        args.length
    );

    // Print navigation help
    if args.page > 1 {
        print!("Use -p {} for previous page. ", args.page - 1);
    }
    if !items.is_empty() && items.len() == args.length as usize {
        print!("Use -p {} for next page.", args.page + 1);
    }
    println!();

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
                length: 35,
                page: 1,
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
