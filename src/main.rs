#![recursion_limit = "1024"]

#[macro_use]
extern crate clap;
#[macro_use]
extern crate error_chain;
extern crate eprompt;
extern crate git2;
extern crate hyper;
extern crate prettytable;
extern crate rpassword;
extern crate rustc_serialize;
extern crate url;
extern crate yaml_rust;

use clap::{App, Arg, ArgMatches};
use std::collections::HashSet;
use std::env;
use std::io;
use std::io::Write;
use std::path::Path;

use eprompt::Prompt;
use rustc_serialize::base64::{ToBase64, STANDARD};

use client::Bitbucket;
use config::Config;
use error::{Error, ErrorKind, Result, UnwrapOrExit};
use pull_request::PullRequest;

mod client;
mod config;
mod error;
mod git;
mod pull_request;
mod util;

pub fn exit(message: &str) -> ! {
    let err = clap::Error::with_description(message, clap::ErrorKind::InvalidValue);
    err.exit();
}

fn prompt(label: &str) -> Result<String> {
    print!("{}", label);
    io::stdout().flush()?; // need to do this since print! won't flush
    let mut res = String::new();
    io::stdin().read_line(&mut res)?;
    Ok(res.trim().to_string())
}

fn setup(path: &Path) -> Result<()> {
    let server = prompt("bitbucket server url: ")?;
    let username = prompt("username: ")?;
    let password = rpassword::prompt_password_stdout("password: ")?;

    println!("
The project name should be the same name as the repo basename (directory).
This enables the auto-detection. If you'd rather specify the project for
a repo explicitly, create a .bitbucket-proj file containing the project
name as specified below");
    let project_name = prompt("primary project name: ")?;

    println!("
The source project is the project KEY or a tilde-prefixed username for
a personal project. This is the project the pull request will be made from.");
    let source_project = prompt("source project: ")?;

    println!("
The source slug is the repository under the source project from which the
pull request will be made.");
    let source_slug = prompt("source slug: ")?;

    println!("
The target project is the project KEY to which the pull request will be made.");
    let target_project = prompt("target project: ")?;

    println!("
The target slug is the repo within the target project to which the pull
request will be made.");
    let target_slug = prompt("target slug: ")?;

    println!("
The target branch is the branch to which the pull request will be made.
This can be overwritten on the command line.");
    let target_branch = prompt("target branch: ")?;

    let auth = format!("{}:{}", username.trim(), password.trim());
    let base64auth = auth.as_bytes().to_base64(STANDARD);

    Config::create_file(path,
                        &server,
                        &base64auth,
                        &project_name,
                        &source_project,
                        &source_slug,
                        &target_project,
                        &target_slug,
                        &target_branch)?;

    println!("
Please edit {} to have your desired configuration (particularly user groups)",
             path.display());

    Ok(())
}

fn groups(config: &Config) -> Result<()> {
    for (name, group) in &config.groups {
        println!("{}: {:?}", name, group);
    }
    Ok(())
}

fn pr(config: &Config, client: &Bitbucket, matches: &ArgMatches) -> Result<()> {
    let debug = matches.is_present("debug");

    let subcmd = matches.subcommand_matches("pr")
        .ok_or::<Error>(ErrorKind::MissingSubcommand("pr".to_string()).into())?;

    let dry = subcmd.is_present("dry_run");

    let project = config.get_project(&util::get_project_name()?)?;

    let title = subcmd.value_of("title").unwrap(); // This is safe since it's required
    let mut description = subcmd.value_of("description").unwrap_or("").to_string();
    if subcmd.is_present("long_description") {
        description = Prompt::new().execute()?.trim().to_string();
    }

    let branch = git::current_branch()?;
    let target_branch = subcmd.value_of("branch").unwrap_or(&project.target_branch);
    let mut pull_request = PullRequest::new(title);
    pull_request.from_ref(&branch, &project.source_slug, &project.source_project);
    pull_request.to_ref(target_branch, &project.target_slug, &project.target_project);
    pull_request.description(&description);

    let mut reviewers = HashSet::new();

    if let Some(reviewer_list) = subcmd.values_of("reviewer") {
        for reviewer in reviewer_list {
            reviewers.insert(reviewer.to_string());
        }
    } else {
        if let Some(groups) = subcmd.values_of("group") {
            for group in groups {
                reviewers = &reviewers | config.get_group(group)?;
            }
        } else {
            reviewers = &reviewers | config.get_group("default")?;
        }

        if let Some(appended) = subcmd.values_of("append") {
            for append in appended {
                reviewers.insert(append.to_string());
            }
        }
    }

    println!("computed reviewers: {:?}", reviewers);

    pull_request.reviewers(reviewers.iter());

    let url = client.create_pull_request(&pull_request, dry, debug)?;

    println!("Created pull request: {}", url.as_str());

    if subcmd.is_present("open") || config.open_in_browser {
        println!("Opening in browser...");
        util::open_in_browser(config, &url)?;
    }

    Ok(())
}

fn main() {
    let default_config_path = env::home_dir().unwrap().join(".bb.yml");
    let yml = load_yaml!("app.yml");
    let matches = App::from_yaml(yml)
        .arg(Arg::with_name("config")
            .help("sets the config file to use")
            .takes_value(true)
            .default_value(default_config_path.to_str().unwrap())
            .short("c")
            .long("config")
            .global(true))
        .get_matches();

    let config_file = matches.value_of("config").unwrap();
    let config_path = Path::new(config_file);

    if matches.is_present("setup") {
        match setup(&config_path) {
            Err(why) => exit(&format!("{}", why)),
            Ok(_) => return,
        }
    }

    let config = Config::from_file(&config_path).unwrap_or_exit("Invalid config file");
    let client = client::Bitbucket::new(config.auth.clone(), config.server.clone())
        .unwrap_or_exit("Could not create client");

    let res = match matches.subcommand_name() {
        Some("groups") => groups(&config),
        Some("pr") => pr(&config, &client, &matches),
        _ => unreachable!(),
    };

    match res {
        Err(why) => exit(&format!("{}", why)),
        Ok(_) => {}
    }
}
