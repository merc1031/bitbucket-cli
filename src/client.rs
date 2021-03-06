use std::io::Read;

use hyper::Client;
use hyper::Url;
use hyper::header::{Authorization, ContentType, Headers};
use hyper::mime::{Attr, Mime, SubLevel, TopLevel, Value};
use serde_json;

use bitbucket_data::{PullRequest, PullRequestList, UserSearchResult};
use config::Project;
use error::{Error, ErrorKind, Result};

pub struct Bitbucket {
    client: Client,
    headers: Headers,
    base_url: Url,
}

impl Bitbucket {
    pub fn new(auth: String, base_url: String) -> Result<Bitbucket> {
        let url = Url::parse(base_url.as_str())?;
        let mut headers = Headers::new();
        headers.set(Authorization(format!("Basic {}", auth).to_owned()));
        headers.set(ContentType(Mime(
            TopLevel::Application,
            SubLevel::Json,
            vec![(Attr::Charset, Value::Utf8)],
        )));
        Ok(Bitbucket {
            client: Client::new(),
            headers: headers,
            base_url: url,
        })
    }

    pub fn branch_exists(&self, project: &Project, branch: &str, debug: bool) -> Result<bool> {
        /*
         * So bitbucket server is really annoying in that they don't let you
         * search for full ref names. Searching for a commit with the ref ends
         * up being a lot more reliable (and faster) than filtering the list
         * of branches from the branch api endpoint.
         */
        let component = format!(
            "rest/api/1.0/projects/{}/repos/{}/commits",
            project.target_project, project.target_slug
        );
        let mut url = self.base_url.join(&component)?;
        url.query_pairs_mut().append_pair("until", branch);
        url.query_pairs_mut().append_pair("limit", "1");

        let mut res = self.client.get(url).headers(self.headers.clone()).send()?;
        let mut response_body = String::new();
        res.read_to_string(&mut response_body)?;

        if debug {
            println!("{}", response_body);
        }

        Ok(res.status.is_success())
    }

    pub fn create_pull_request(
        &self,
        pull_request: &PullRequest,
        dry: bool,
        debug: bool,
    ) -> Result<Url> {
        let component = format!(
            "rest/api/1.0/projects/{}/repos/{}/pull-requests",
            pull_request
                .project()
                .ok_or::<Error>(ErrorKind::InvalidPullRequest("Missing toRef".to_string()).into())?,
            pull_request
                .slug()
                .ok_or::<Error>(ErrorKind::InvalidPullRequest("Missing toRef".to_string()).into())?
        );
        let url = self.base_url.join(&component)?;
        let body = serde_json::to_string(pull_request)?;

        if debug {
            println!("{}", body);
        }

        if dry {
            println!("Dry run: \"{}\"", body);
            return Err(ErrorKind::DryRun.into());
        }

        let mut res = self.client
            .post(url)
            .headers(self.headers.clone())
            .body(body.as_str())
            .send()?;
        let mut response_body = String::new();
        res.read_to_string(&mut response_body)?;

        if res.status.is_success() {
            if debug {
                println!("{}", response_body);
            }
            let data = serde_json::from_str(response_body.as_str())?;
            get_self_url(&data)
        } else {
            Err(ErrorKind::RequestError(response_body).into())
        }
    }

    pub fn list_pull_requests(&self, debug: bool, role: &str) -> Result<PullRequestList> {
        let mut url = self.base_url.join("rest/api/1.0/dashboard/pull-requests")?;
        url.query_pairs_mut().append_pair("state", "OPEN");

        // annoyingly, the only way to specify this behavior is to not include
        // the parameter at all
        if role != "ALL" {
            url.query_pairs_mut().append_pair("role", role);
        }

        if debug {
            println!("{}", url);
        }

        let mut res = self.client.get(url).headers(self.headers.clone()).send()?;

        let mut response_body = String::new();
        res.read_to_string(&mut response_body)?;
        if res.status.is_success() {
            if debug {
                println!("{}", response_body);
            }
            let res = serde_json::from_str(response_body.as_str())?;
            Ok(res)
        } else {
            Err(ErrorKind::RequestError(response_body).into())
        }
    }

    pub fn user(&self, filter: &str, debug: bool) -> Result<UserSearchResult> {
        let mut url = self.base_url.join("rest/api/1.0/users")?;
        url.query_pairs_mut().append_pair("filter", filter);

        if debug {
            println!("{}", url);
        }

        let mut res = self.client.get(url).headers(self.headers.clone()).send()?;

        let mut response_body = String::new();
        res.read_to_string(&mut response_body)?;
        if res.status.is_success() {
            if debug {
                println!("{}", response_body);
            }
            let res = serde_json::from_str(response_body.as_str())?;
            Ok(res)
        } else {
            Err(ErrorKind::RequestError(response_body).into())
        }
    }
}

fn get_self_url(pull_request: &PullRequest) -> Result<Url> {
    if let Some(link) = pull_request.self_link() {
        let url = Url::parse(&link)?;
        Ok(url)
    } else {
        Err(ErrorKind::MissingSelfLink.into())
    }
}
