//! Open pull requests from GitHub's REST API (github.com only; no GitHub Enterprise yet).
//!
//! The pull requests shown are those the user can act on: the open pull requests of the
//! repository `origin` points at and, if that is a fork, the fork's own pull requests into its
//! parent (research §12).
//!
//! Signing in is optional: if the `gh` program is installed and signed in, its token is used,
//! which raises the limit from 60 to 5000 requests an hour and reaches private repositories.
//! Without it, requests go unauthenticated. Tokens from `GH_TOKEN` and git's credential helpers
//! (research §6) can be added later.

use std::fmt;

use super::{ForgeError, PullRequest, PullRequests, Remote};
use crate::git::Git;

/// Where the REST API is. Tokens are only ever sent here.
const API: &str = "https://api.github.com/";
const WEB: &str = "https://github.com/";
/// Pages of 100 pull requests fetched at most per repository: a guard against a server that
/// keeps linking to a next page, not a cap (no repository has 10,000 open pull requests).
const MAX_PAGES: usize = 100;

/// A repository on github.com.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubRepo {
    pub owner: String,
    pub name: String,
}

impl GithubRepo {
    /// The repository a remote URL points at, if it is on github.com: `https://github.com/o/r`,
    /// `git@github.com:o/r.git`, `ssh://git@ssh.github.com:443/o/r` and the like.
    pub fn from_url(url: &str) -> Option<GithubRepo> {
        let url = url.trim();
        let (host, path) = match url.split_once("://") {
            Some((scheme, rest)) => {
                let scheme = scheme.to_ascii_lowercase();
                let scheme = scheme.strip_prefix("git+").unwrap_or(&scheme);
                if !matches!(scheme, "https" | "http" | "ssh" | "git") {
                    return None;
                }
                let (authority, path) = rest.split_once('/')?;
                let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
                let host = host.split_once(':').map_or(host, |(h, _)| h);
                (host, path)
            }
            // scp-like: [user@]host:path, with no slash before the colon.
            None => {
                let (authority, path) = url.split_once(':')?;
                if authority.contains('/') {
                    return None;
                }
                let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
                (host, path)
            }
        };
        let host = host.to_ascii_lowercase();
        if !matches!(
            host.as_str(),
            "github.com" | "www.github.com" | "ssh.github.com"
        ) {
            return None;
        }
        let path = path.trim_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        let (owner, name) = path.split_once('/')?;
        // Enterprise Managed Users' logins end in `_shortcode`.
        let owner_ok = !owner.is_empty()
            && owner
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
        let name_ok = !name.is_empty()
            && name != "."
            && name != ".."
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        (owner_ok && name_ok).then(|| GithubRepo {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }

    /// `owner/name`, from a full name the API gave; `None` if it doesn't look like one.
    fn from_full_name(full_name: &str) -> Option<GithubRepo> {
        GithubRepo::from_url(&format!("{WEB}{full_name}"))
    }

    /// `owner/name`.
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// The GitHub repository `origin` points at, if any. Asks git only, not GitHub.
pub fn origin(git: &Git) -> Option<GithubRepo> {
    let url = git.query(&["remote", "get-url", "origin"]).ok()??;
    GithubRepo::from_url(&url)
}

/// Loads the open pull requests for the repository `git` works on (see the module docs). Takes
/// a request per 100 pull requests, plus one; run it on a worker thread.
pub fn load(git: &Git) -> Result<PullRequests, ForgeError> {
    let urls = super::remote_urls(git)?;
    let origin = urls
        .iter()
        .find(|(name, _)| name == "origin")
        .and_then(|(_, url)| GithubRepo::from_url(url))
        .ok_or(ForgeError::NoForge)?;
    let upstreams = super::upstreams(git)?;
    let (canonical, list) = with_api(|api| open_pull_requests(api, &origin))?;
    let remotes = urls
        .iter()
        .filter_map(|(name, url)| {
            let repo = GithubRepo::from_url(url)?;
            // A renamed repository answers under its new name; remotes may still use the old.
            let repo = if repo.full_name().eq_ignore_ascii_case(&origin.full_name()) {
                canonical.clone()
            } else {
                repo.full_name()
            };
            Some(Remote {
                name: name.clone(),
                repo,
            })
        })
        .collect();
    Ok(PullRequests {
        list,
        remotes,
        upstreams,
    })
}

/// One page of an API answer: its body, and the URL of the next page if there is one.
#[derive(Debug)]
#[cfg_attr(not(feature = "github"), allow(dead_code))]
pub(crate) struct Page {
    pub body: String,
    pub next: Option<String>,
}

/// GET requests against the REST API; a trait so that tests can answer them.
pub(crate) trait Api {
    fn get(&self, url: &str) -> Result<Page, ForgeError>;
}

/// The repository's `owner/name` as GitHub has it now, and the pull requests of `origin`,
/// plus its own ones into its parent if it is a fork. Newest first, `origin`'s first.
pub(crate) fn open_pull_requests(
    api: &dyn Api,
    origin: &GithubRepo,
) -> Result<(String, Vec<PullRequest>), ForgeError> {
    let info: json::Repository = parse(&api.get(&format!("{API}repos/{}", origin.full_name()))?)?;
    let mut list = pull_requests(api, &info.full_name)?;
    if let Some(parent) = &info.parent {
        let ours = pull_requests(api, &parent.full_name)?
            .into_iter()
            .filter(|pr| {
                pr.head_repo
                    .as_deref()
                    .is_some_and(|r| r.eq_ignore_ascii_case(&info.full_name))
            });
        list.extend(ours);
    }
    Ok((info.full_name, list))
}

/// Every open pull request of the repository `full_name`, newest first.
fn pull_requests(api: &dyn Api, full_name: &str) -> Result<Vec<PullRequest>, ForgeError> {
    let repo = GithubRepo::from_full_name(full_name)
        .ok_or_else(|| ForgeError::Parse(format!("odd repository name {full_name:?}")))?;
    let mut url = format!(
        "{API}repos/{}/pulls?state=open&per_page=100",
        repo.full_name()
    );
    let mut list = Vec::new();
    for _ in 0..MAX_PAGES {
        let page = api.get(&url)?;
        let prs: Vec<json::PullRequest> = parse(&page)?;
        list.extend(prs.into_iter().filter_map(|pr| pr.into_pull_request(&repo)));
        match page.next {
            // Only ever to the API: a token must not follow a link elsewhere.
            Some(next) if next.starts_with(API) => url = next,
            _ => break,
        }
    }
    Ok(list)
}

#[cfg(feature = "github")]
fn parse<T: serde::de::DeserializeOwned>(page: &Page) -> Result<T, ForgeError> {
    serde_json::from_str(&page.body).map_err(|e| ForgeError::Parse(e.to_string()))
}

#[cfg(not(feature = "github"))]
fn parse<T>(_: &Page) -> Result<T, ForgeError> {
    Err(ForgeError::Unsupported)
}

/// The `rel="next"` URL of a `Link` header.
#[cfg_attr(not(feature = "github"), allow(dead_code))]
fn next_link(link: &str) -> Option<String> {
    link.split(',').find_map(|part| {
        let (url, params) = part.split_once(';')?;
        let next = params
            .split(';')
            .any(|p| p.trim().replace(' ', "") == "rel=\"next\"");
        let url = url.trim().strip_prefix('<')?.strip_suffix('>')?;
        next.then(|| url.to_owned())
    })
}

/// The API's JSON, as far as it is read.
mod json {
    use serde::Deserialize;

    use super::{GithubRepo, WEB};
    use crate::oid::Oid;

    #[derive(Debug, Deserialize)]
    pub struct Repository {
        pub full_name: String,
        /// Present when the repository is a fork.
        pub parent: Option<Parent>,
    }

    #[derive(Debug, Deserialize)]
    pub struct Parent {
        pub full_name: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct PullRequest {
        number: u64,
        title: String,
        #[serde(default)]
        draft: bool,
        user: Option<User>,
        head: Branch,
        base: Branch,
    }

    #[derive(Debug, Deserialize)]
    struct User {
        login: String,
    }

    #[derive(Debug, Deserialize)]
    struct Branch {
        #[serde(rename = "ref")]
        name: String,
        sha: String,
        repo: Option<Parent>,
    }

    impl PullRequest {
        /// The pull request of the repository `repo`, if its head is a commit id. Its page is
        /// built here, not taken from the answer, so that only github.com is ever opened.
        pub fn into_pull_request(self, repo: &GithubRepo) -> Option<crate::forge::PullRequest> {
            Some(crate::forge::PullRequest {
                number: self.number,
                title: self.title,
                author: self.user.map(|u| u.login).unwrap_or_default(),
                draft: self.draft,
                head: Oid::from_hex(&self.head.sha)?,
                head_branch: self.head.name,
                head_repo: self.head.repo.map(|r| r.full_name),
                base_branch: self.base.name,
                base_repo: repo.full_name(),
                url: format!("{WEB}{}/pull/{}", repo.full_name(), self.number),
            })
        }
    }
}

/// A token for the API. Never printed: its `Debug` shows stars.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Token(String);

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(***)")
    }
}

/// The token of a signed-in `gh` (`gh auth token`), if `gh` is installed and answers within a
/// few seconds.
#[cfg_attr(not(feature = "github"), allow(dead_code))]
fn gh_token() -> Option<Token> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut cmd = Command::new("gh");
    cmd.args(["auth", "token", "--hostname", "github.com"])
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        // As for git: no console window flashing up.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    let token = out.trim();
    // Tokens are letters, digits and underscores; anything else is not one.
    let plausible = !token.is_empty()
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_');
    plausible.then(|| Token(token.to_owned()))
}

/// Runs `calls` against the API, with `gh`'s token if there is one. If GitHub turns the token
/// down (401), or refuses it for a repository (403, e.g. an organisation that requires single
/// sign-on), runs them again without it.
#[cfg(feature = "github")]
fn with_api<T>(calls: impl Fn(&dyn Api) -> Result<T, ForgeError>) -> Result<T, ForgeError> {
    let token = gh_token();
    let signed_in = Http::new(token.clone());
    match calls(&signed_in) {
        Err(ForgeError::Status {
            status: 401 | 403, ..
        }) if token.is_some() => calls(&Http::new(None)),
        result => result,
    }
}

#[cfg(not(feature = "github"))]
fn with_api<T>(_: impl Fn(&dyn Api) -> Result<T, ForgeError>) -> Result<T, ForgeError> {
    Err(ForgeError::Unsupported)
}

/// The API over HTTPS, with ureq.
#[cfg(feature = "github")]
struct Http {
    agent: ureq::Agent,
    token: Option<Token>,
}

#[cfg(feature = "github")]
impl Http {
    fn new(token: Option<Token>) -> Http {
        use std::time::Duration;
        use ureq::tls::{RootCerts, TlsConfig};
        // The system's certificate authorities, including any a company adds.
        let tls = TlsConfig::builder()
            .root_certs(RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .timeout_global(Some(Duration::from_secs(30)))
            // Error answers carry the reason, and the rate limit in their headers.
            .http_status_as_error(false)
            .redirect_auth_headers(ureq::config::RedirectAuthHeaders::SameHost)
            .user_agent(concat!("parterre/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Http { agent, token }
    }
}

#[cfg(feature = "github")]
impl fmt::Debug for Http {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Http")
            .field("token", &self.token)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "github")]
impl Api for Http {
    fn get(&self, url: &str) -> Result<Page, ForgeError> {
        let mut request = self
            .agent
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(Token(token)) = &self.token
            && url.starts_with(API)
        {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        let mut response = request
            .call()
            .map_err(|e| ForgeError::Network(e.to_string()))?;
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let status = response.status().as_u16();
        let next = header("link").as_deref().and_then(next_link);
        let remaining = header("x-ratelimit-remaining");
        let limit = header("x-ratelimit-limit").and_then(|l| l.parse().ok());
        let reset = header("x-ratelimit-reset").and_then(|r| r.parse::<u64>().ok());
        let body = response
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| ForgeError::Network(e.to_string()))?;
        let authenticated = self.token.is_some();
        match status {
            200..=299 => Ok(Page { body, next }),
            403 | 429 if remaining.as_deref() == Some("0") => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                Err(ForgeError::RateLimited {
                    limit: limit.unwrap_or(60),
                    minutes: reset.map_or(60, |r| r.saturating_sub(now).div_ceil(60)),
                    authenticated,
                })
            }
            404 => Err(ForgeError::NotFound {
                repo: url
                    .strip_prefix(API)
                    .and_then(|p| p.strip_prefix("repos/"))
                    .map_or(url, |p| p.split(['?']).next().unwrap_or(p))
                    .trim_end_matches("/pulls")
                    .to_owned(),
                authenticated,
            }),
            _ => {
                #[derive(serde::Deserialize)]
                struct Message {
                    message: String,
                }
                let message = serde_json::from_str::<Message>(&body)
                    .map(|m| m.message)
                    .unwrap_or_else(|_| body.chars().take(200).collect());
                Err(ForgeError::Status { status, message })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(owner: &str, name: &str) -> Option<GithubRepo> {
        Some(GithubRepo {
            owner: owner.into(),
            name: name.into(),
        })
    }

    #[test]
    fn github_remote_urls() {
        for url in [
            "https://github.com/aquamoth/parterre",
            "https://github.com/aquamoth/parterre.git",
            "https://github.com/aquamoth/parterre/",
            "https://user@github.com/aquamoth/parterre.git",
            "HTTPS://GitHub.com/aquamoth/parterre",
            "http://www.github.com/aquamoth/parterre",
            "git+https://github.com/aquamoth/parterre",
            "ssh://git@github.com/aquamoth/parterre.git",
            "ssh://git@ssh.github.com:443/aquamoth/parterre.git",
            "git://github.com/aquamoth/parterre",
            "git@github.com:aquamoth/parterre.git",
            "github.com:aquamoth/parterre",
            " git@github.com:aquamoth/parterre \n",
        ] {
            assert_eq!(
                GithubRepo::from_url(url),
                repo("aquamoth", "parterre"),
                "{url}"
            );
        }
        assert_eq!(
            GithubRepo::from_url("git@github.com:my-org/some.repo_name.git"),
            repo("my-org", "some.repo_name")
        );
        assert_eq!(
            GithubRepo::from_url("https://github.com/jdoe_acme/r"),
            repo("jdoe_acme", "r")
        );
    }

    #[test]
    fn other_remote_urls() {
        for url in [
            "https://gitlab.com/aquamoth/parterre",
            "https://github.com.evil.example/aquamoth/parterre",
            "https://dev.azure.com/org/project/_git/repo",
            "git@bitbucket.org:aquamoth/parterre.git",
            "/home/me/src/parterre",
            "../parterre",
            "C:\\src\\parterre",
            "file:///srv/github.com/aquamoth/parterre",
            "https://github.com/aquamoth",
            "https://github.com/aquamoth/parterre/tree/main",
            "https://github.com/aquamoth/..",
            "https://github.com/aqua moth/parterre",
            "https://github.com/aquamoth/parterre?x=1",
            "",
        ] {
            assert_eq!(GithubRepo::from_url(url), None, "{url}");
        }
    }

    #[test]
    fn next_links() {
        let link = "<https://api.github.com/repositories/1/pulls?page=2>; rel=\"next\", \
                    <https://api.github.com/repositories/1/pulls?page=5>; rel=\"last\"";
        assert_eq!(
            next_link(link).as_deref(),
            Some("https://api.github.com/repositories/1/pulls?page=2")
        );
        let last = "<https://api.github.com/repositories/1/pulls?page=4>; rel=\"prev\", \
                    <https://api.github.com/repositories/1/pulls?page=1>; rel=\"first\"";
        assert_eq!(next_link(last), None);
        assert_eq!(next_link(""), None);
    }

    #[test]
    fn tokens_are_not_printed() {
        let token = Token("gho_secret".into());
        assert_eq!(format!("{token:?}"), "Token(***)");
        assert!(!format!("{:?}", Some(token)).contains("secret"));
    }

    /// Answers from a map of URL → (body, next page).
    #[cfg(feature = "github")]
    struct Fake {
        pages: std::collections::HashMap<String, (String, Option<String>)>,
        asked: std::cell::RefCell<Vec<String>>,
    }

    #[cfg(feature = "github")]
    impl Fake {
        fn new(pages: &[(&str, &str, Option<&str>)]) -> Fake {
            Fake {
                pages: pages
                    .iter()
                    .map(|(url, body, next)| {
                        (
                            (*url).to_owned(),
                            ((*body).to_owned(), next.map(str::to_owned)),
                        )
                    })
                    .collect(),
                asked: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    #[cfg(feature = "github")]
    impl Api for Fake {
        fn get(&self, url: &str) -> Result<Page, ForgeError> {
            self.asked.borrow_mut().push(url.to_owned());
            let (body, next) = self.pages.get(url).cloned().ok_or(ForgeError::NotFound {
                repo: url.into(),
                authenticated: false,
            })?;
            Ok(Page { body, next })
        }
    }

    #[cfg(feature = "github")]
    fn pr_json(number: u64, head_repo: &str, head: char, draft: bool) -> String {
        format!(
            r#"{{"number":{number},"title":"PR {number}","draft":{draft},
                "html_url":"https://evil.example/{number}","user":{{"login":"someone"}},
                "head":{{"ref":"topic-{number}","sha":"{sha}","repo":{{"full_name":"{head_repo}"}}}},
                "base":{{"ref":"main","sha":"{base}","repo":{{"full_name":"x/y"}}}},
                "unknown":[1,2,3]}}"#,
            sha = head.to_string().repeat(40),
            base = "0".repeat(40),
        )
    }

    #[cfg(feature = "github")]
    #[test]
    fn lists_open_pull_requests_page_by_page() {
        let page1 = format!(
            "[{},{}]",
            pr_json(12, "me/repo", 'a', false),
            pr_json(11, "someone/fork", 'b', true)
        );
        let page2 = format!("[{}]", pr_json(3, "me/repo", 'c', false));
        let api = Fake::new(&[
            (
                "https://api.github.com/repos/Me/Repo",
                r#"{"full_name":"me/repo","fork":false,"parent":null}"#,
                None,
            ),
            (
                "https://api.github.com/repos/me/repo/pulls?state=open&per_page=100",
                &page1,
                Some("https://api.github.com/repositories/7/pulls?page=2"),
            ),
            (
                "https://api.github.com/repositories/7/pulls?page=2",
                &page2,
                // Never followed off the API.
                Some("https://evil.example/pulls?page=3"),
            ),
        ]);
        let (name, list) = open_pull_requests(&api, &repo("Me", "Repo").unwrap()).unwrap();
        assert_eq!(name, "me/repo");
        let numbers: Vec<u64> = list.iter().map(|pr| pr.number).collect();
        assert_eq!(numbers, [12, 11, 3]);
        let pr = &list[1];
        assert_eq!(pr.title, "PR 11");
        assert_eq!(pr.author, "someone");
        assert!(pr.draft);
        assert_eq!(pr.head.to_hex(), "b".repeat(40));
        assert_eq!(pr.head_branch, "topic-11");
        assert_eq!(pr.head_repo.as_deref(), Some("someone/fork"));
        assert_eq!(pr.base_branch, "main");
        assert_eq!(pr.base_repo, "me/repo");
        // Built from the repository, not the answer's html_url.
        assert_eq!(pr.url, "https://github.com/me/repo/pull/11");
        assert_eq!(api.asked.borrow().len(), 3);
    }

    #[cfg(feature = "github")]
    #[test]
    fn a_fork_gets_its_own_pull_requests_into_its_parent() {
        let own = format!("[{}]", pr_json(2, "me/fork", 'a', false));
        let parent = format!(
            "[{},{},{}]",
            pr_json(40, "other/fork", 'b', false),
            pr_json(39, "Me/Fork", 'c', false),
            // Its fork was deleted.
            pr_json(38, "", 'd', false).replace(r#"{"full_name":""}"#, "null"),
        );
        let api = Fake::new(&[
            (
                "https://api.github.com/repos/me/fork",
                r#"{"full_name":"me/fork","fork":true,"parent":{"full_name":"up/stream"}}"#,
                None,
            ),
            (
                "https://api.github.com/repos/me/fork/pulls?state=open&per_page=100",
                &own,
                None,
            ),
            (
                "https://api.github.com/repos/up/stream/pulls?state=open&per_page=100",
                &parent,
                None,
            ),
        ]);
        let (_, list) = open_pull_requests(&api, &repo("me", "fork").unwrap()).unwrap();
        let found: Vec<(u64, &str)> = list
            .iter()
            .map(|pr| (pr.number, pr.base_repo.as_str()))
            .collect();
        assert_eq!(found, [(2, "me/fork"), (39, "up/stream")]);
        assert_eq!(list[1].url, "https://github.com/up/stream/pull/39");
    }

    #[cfg(feature = "github")]
    #[test]
    fn errors_are_passed_on() {
        let api = Fake::new(&[]);
        assert!(matches!(
            open_pull_requests(&api, &repo("me", "gone").unwrap()),
            Err(ForgeError::NotFound { .. })
        ));
        let api = Fake::new(&[("https://api.github.com/repos/me/odd", "<html>", None)]);
        assert!(matches!(
            open_pull_requests(&api, &repo("me", "odd").unwrap()),
            Err(ForgeError::Parse(_))
        ));
    }
}
