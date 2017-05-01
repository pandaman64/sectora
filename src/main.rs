use std::fs::File;
use std::io::prelude::*;
use std::collections::HashMap;

extern crate clap;
use clap::{Arg, App, SubCommand};
extern crate toml;

// extern crate hyper;
// extern crate hyper_rustls;
// use hyper::Client;
// use hyper::net::HttpsConnector;
// use hyper_rustls::TlsClient;
// use hyper::header::Authorization;
// use hyper::header::{Accept, qitem};
// use hyper::mime::{Mime, TopLevel, SubLevel};

extern crate reqwest;
use reqwest::header::{Authorization,Accept};

extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;


#[derive(Deserialize, Debug)]
struct Config {
    token: String,
    org: String,
    team: String,
    #[serde(default="default_endpoint")]
    endpoint: String,
    #[serde(default="default_home")]
    home: String,
    #[serde(default="default_gid")]
    gid: u64,
    #[serde(default="default_sh")]
    sh: String,
    group: Option<String>,
}

fn default_endpoint() -> String { String::from("https://api.github.com") }
fn default_home() -> String { String::from("/home/{}") }
fn default_gid() -> u64 { 2000 }
fn default_sh() -> String { String::from("/bin/bash") }

#[derive(Serialize, Deserialize, Debug)]
struct Team {
    id: u64,
    name: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Member {
    id: u64,
    login: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct PublicKey {
    id: u64,
    key: String,
}

#[derive(Debug)]
enum CliError {
    Serde(serde_json::Error),
    Reqwest(reqwest::Error),
    Io(std::io::Error)
}

impl From<serde_json::Error> for CliError {fn from(err: serde_json::Error) -> CliError { CliError::Serde(err) }}
impl From<reqwest::Error> for CliError {fn from(err: reqwest::Error) -> CliError { CliError::Reqwest(err) }}
impl From<std::io::Error> for CliError {fn from(err: std::io::Error) -> CliError { CliError::Io(err) }}

fn main() {

    let matches = App::new("ghteam-auth")
                      .version("0.1")
                      .author("Yasuyuki YAMADA <yasuyuki.ymd@gmail.com>")
                      .about("")
                      .arg(Arg::with_name("config")
                               .short("c")
                               .long("config")
                               .value_name("FILE")
                               .help("Sets a custom config file (toml)")
                               .takes_value(true))
                      .arg(Arg::with_name("v")
                               .short("v")
                               .multiple(true)
                               .help("Sets the level of verbosity"))
                      .subcommand(SubCommand::with_name("key")
                                             .about("get user public key")
                                             .arg(Arg::with_name("USER")
                                                      .required(true)
                                                      .index(1)
                                                      .help("user name")))
                      .subcommand(SubCommand::with_name("pam")
                                             .about("get user public key")
                                             .arg(Arg::with_name("USER")
                                                      .required(false)
                                                      .index(1)
                                                      .help("user name")))
                      .subcommand(SubCommand::with_name("passwd")
                                             .about("get passwd"))
                      .subcommand(SubCommand::with_name("shadow")
                                             .about("get shadow"))
                      .subcommand(SubCommand::with_name("group")
                                             .about("get group"))
                      .subcommand(SubCommand::with_name("refresh")
                                             .about("refresh cache"))
                      .get_matches();

    let config = matches.value_of("config").unwrap_or("/etc/ghteam-auth.conf");
    let mut file = File::open(config).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let config = toml::from_str::<Config>(contents.as_str()).unwrap();

    // let client = Client::with_connector(HttpsConnector::new(TlsClient::new()));
    let client = reqwest::Client::new().unwrap();
    let client = GithubClient::new(client, config);

    if let Some(matches) = matches.subcommand_matches("key") {
        client.print_user_public_key(matches.value_of("USER").unwrap());
    } else if let Some(_) = matches.subcommand_matches("passwd") {
        client.get_passwd().unwrap();
    } else if let Some(_) = matches.subcommand_matches("shadow") {
        client.get_shadow().unwrap();
    } else if let Some(_) = matches.subcommand_matches("group") {
        client.get_group().unwrap();
    } else if let Some(_) = matches.subcommand_matches("pam") {
        match std::env::var("PAM_USER") {
            Ok(user) => {
                if client.check_pam(&user).unwrap() { std::process::exit(0); }
                else { std::process::exit(1) }
            }
            Err(e) => println!("couldn't interpret PAM_USER: {}", e),
        }
    }

}

struct GithubClient {
    // client: hyper::Client,
    client: reqwest::Client,
    conf: Config
}

impl GithubClient {
    fn new(client:reqwest::Client, conf:Config) -> GithubClient {
        GithubClient {client:client, conf:conf}
    }

    fn get_url_content(&self, url:&String) -> Result<String,CliError> {
        // println!("GET {}", url);
        let token = String::from("token ") + self.conf.token.clone().as_str();
        let res = self.client.get(url.as_str()).header(Authorization(token)).send();
        let mut content = String::new();
        res?.read_to_string(&mut content)?;
        Ok(content)
    }

    fn print_user_public_key(&self, user:&str) -> Result<(), CliError> {
        let keys = self.get_user_public_key(user)?;
        println!("{}", keys);
        Ok(())
    }

    fn get_user_public_key(&self, user:&str) -> Result<String, CliError> {
        let url = format!("{}/users/{}/keys", self.conf.endpoint.clone(),user);
        let content = self.get_url_content(&url);
        let keys = serde_json::from_str::<Vec<PublicKey>>(content?.as_str())?;
        Ok(keys.iter().map(|k|{k.key.clone()}).collect::<Vec<String>>().join("\n"))
    }

    fn check_pam(&self, user:&String) -> Result<bool, CliError> {
        let teams:HashMap<String,Team> = self.get_teams()?;
        if let Some(team) = teams.get(&self.conf.team.clone()) {
            for member in self.get_members(team.id)? {
                if member.login==*user { return Ok(true) }
            }
        }
        Ok(false)
    }

    fn get_passwd(&self) -> Result<(), CliError> {
        let teams:HashMap<String,Team> = self.get_teams()?;
        if let Some(team) = teams.get(&self.conf.team.clone()) {
            for member in self.get_members(team.id)? {
                println!("{}", self.create_passwd_line(&member));
            }
            Ok(())
        } else {
            Ok(())
        }
    }

    fn get_shadow(&self) -> Result<(), CliError> {
        let teams:HashMap<String,Team> = self.get_teams()?;
        if let Some(team) = teams.get(&self.conf.team.clone()) {
            for member in self.get_members(team.id)? {
                println!("{}", self.create_shadow_line(&member));
            }
            Ok(())
        } else {
            Ok(())
        }
    }

    fn get_group(&self) -> Result<(), CliError> {
        let teams:HashMap<String,Team> = self.get_teams()?;
        if let Some(team) = teams.get(&self.conf.team.clone()) {
            let members = self.get_members(team.id)?;
            println!("{}", self.create_group_line(&team.name, self.conf.gid, &members));
            Ok(())
        } else {
            Ok(())
        }
    }

    fn create_passwd_line(&self, member:&Member) -> String {
        format!("{login}:x:{uid}:{gid}:user@{org}:{home}:{sh}",
                login=member.login,
                uid=member.id,
                gid=self.conf.gid,
                org=self.conf.org,
                home=self.conf.home.replace("{}",member.login.as_str()),
                sh=self.conf.sh,
                )
    }

    fn create_shadow_line(&self, member:&Member) -> String {
        format!("{login}:!!:::::::", login=member.login )
    }

    fn create_group_line(&self, name:&String, id:u64, members:&Vec<Member>) -> String {
        format!("{name}:x:{id}:{members}", name=name, id=id,
                members=members.iter().map(|m|{m.login.clone()}).collect::<Vec<String>>().join(","))
    }

    fn get_teams(&self) -> Result<HashMap<String,Team>, CliError> {
        let url = format!("{}/orgs/{}/teams",self.conf.endpoint.clone(), self.conf.org.clone());
        let content = self.get_url_content(&url)?;
        let teams = serde_json::from_str::<Vec<Team>>(content.as_str())?;
        let mut team_map = HashMap::new();
        for team in teams { team_map.insert(team.name.clone(), team); }
        Ok(team_map)
    }

    fn get_members(&self, mid:u64) -> Result<Vec<Member>, CliError> {
        let url = format!("{}/teams/{}/members",self.conf.endpoint.clone(), mid);
        let content = self.get_url_content(&url)?;
        let members = serde_json::from_str::<Vec<Member>>(content.as_str())?;
        Ok(members)
    }


}
