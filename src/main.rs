#[macro_use]
extern crate clap;
use clap::{App, Arg, SubCommand};
extern crate glob;
extern crate reqwest;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate toml;

#[macro_use]
extern crate lazy_static;

extern crate libc;

mod structs;
mod ghclient;
mod statics;
use statics::CLIENT;

macro_rules! syslog {
    ($msg:expr) => (
        unsafe {
            libc::openlog("sectora".as_ptr() as *const i8, libc::LOG_PID, libc::LOG_AUTH);
            libc::syslog(libc::LOG_NOTICE, $msg.as_ptr() as *const i8);
            libc::closelog();
        }
    )
}

fn main() {
    let user_arg = Arg::with_name("USER").required(true)
                                         .index(1)
                                         .help("user name");
    let conf_arg = Arg::with_name("CONF").required(true)
                                         .index(1)
                                         .help("conf path");
    let app = App::new(crate_name!()).version(crate_version!())
                                     .author(crate_authors!())
                                     .about(crate_description!())
                                     .subcommand(SubCommand::with_name("key").about("Gets user public key")
                                                                             .arg(user_arg))
                                     .subcommand(SubCommand::with_name("pam").about("Executes pam check"))
                                     .subcommand(SubCommand::with_name("cleanup").about("Cleans caches up"))
                                     .subcommand(SubCommand::with_name("check").about("Check configuration")
                                                                               .arg(conf_arg))
                                     .get_matches();

    use std::env;
    use std::path::Path;
    use std::process;

    match app.subcommand() {
        ("key", Some(sub)) => match CLIENT.print_user_public_key(sub.value_of("USER").unwrap()) {
            Ok(_) => {
                syslog!("sectora key (success).");
                process::exit(0)
            }
            Err(_) => {
                syslog!("sectora key (fail).");
                process::exit(21)
            }
        },
        ("check", Some(sub)) => match structs::Config::new(Path::new(sub.value_of("CONF").unwrap())) {
            Ok(_) => process::exit(0),
            Err(_) => process::exit(11),
        },
        ("cleanup", Some(_)) => CLIENT.clear_all_caches().unwrap(),
        ("pam", Some(_)) => match env::var("PAM_USER") {
            Ok(user) => {
                if CLIENT.check_pam(&user).unwrap() {
                    syslog!("sectora pam (success).");
                    process::exit(0);
                } else {
                    syslog!("sectora pam (fail).");
                    process::exit(1)
                }
            }
            Err(e) => println!("PAM_USER: {}", e),
        },
        (&_, _) => println!("{}", app.usage()),
    }
}
