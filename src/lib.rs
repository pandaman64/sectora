extern crate toml;
extern crate reqwest;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate glob;
extern crate nix;

#[macro_use]
extern crate lazy_static;

use std::io::{Read,Write,Seek,SeekFrom,BufReader,BufRead};
use std::fs::{File,OpenOptions};
use std::ffi::CStr;
extern crate libc;

mod structs;
mod ghclient;
use ghclient::GithubClient;
mod buffer;
use buffer::Buffer;
mod cstructs;
use cstructs::{Passwd,Spwd,Group};

lazy_static! {
    static ref CLIENT:GithubClient = GithubClient::new(
        std::env::var("GHTEAMAUTH_CONFIG")
                 .unwrap_or(String::from("/etc/ghteam-auth.conf"))
                 .as_str()
    ).unwrap();
}

const RUN_DIR:&str = "/var/run/ghteam-auth";

#[allow(dead_code)]
enum NssStatus {
    TryAgain,
    Unavail,
    NotFound,
    Success,
}

impl From<NssStatus> for libc::c_int {
    fn from(status:NssStatus) -> libc::c_int {
        match status {
            NssStatus::TryAgain => -2,
            NssStatus::Unavail  => -1,
            NssStatus::NotFound => 0,
            NssStatus::Success  => 1,
        }
    }
}

#[no_mangle]
pub extern "C" fn _nss_ghteam_getpwnam_r(cnameptr: *const libc::c_char,
                                         pw: *mut Passwd,
                                         buf: *mut libc::c_char,
                                         buflen: libc::size_t,
                                         _: *mut libc::c_int) -> libc::c_int {
    let mut buffer = Buffer::new(buf,buflen);
    let cname: &CStr = unsafe {CStr::from_ptr(cnameptr)};
    let name = String::from(cname.to_str().unwrap());
    let (_,members) = CLIENT.get_team_members().unwrap();
    match members.get(&name) {
        Some(member) => {
            match unsafe{(*pw).pack(
                    &mut buffer,
                    &member.login,
                    "x",
                    member.id as libc::uid_t,
                    CLIENT.conf.gid as libc::gid_t,
                    "",
                    &CLIENT.conf.home.replace("{}",member.login.as_str()),
                    &CLIENT.conf.sh,
            )} {
                Ok(_) => libc::c_int::from(NssStatus::Success),
                Err(_) => nix::Errno::ERANGE as libc::c_int
            }
        },
        None => libc::c_int::from(NssStatus::NotFound)
    }
}

#[no_mangle]
pub extern "C" fn _nss_ghteam_getpwuid_r(uid: libc::uid_t,
                                         pw: *mut Passwd,
                                         buf: *mut libc::c_char,
                                         buflen: libc::size_t,
                                         _: *mut libc::c_int) -> libc::c_int {
    let mut buffer = Buffer::new(buf,buflen);
    let (_,members) = CLIENT.get_team_members().unwrap();
    for member in members.values() {
        if uid == member.id as libc::uid_t {
            match unsafe {(*pw).pack(
                    &mut buffer,
                    &member.login,
                    "x",
                    member.id as libc::uid_t,
                    CLIENT.conf.gid as libc::gid_t,
                    "",
                    &CLIENT.conf.home.replace("{}",member.login.as_str()),
                    &CLIENT.conf.sh,
                )} {
                Ok(_) => return libc::c_int::from(NssStatus::Success),
                Err(_) => return nix::Errno::ERANGE as libc::c_int
            }
        }
    }
    libc::c_int::from(NssStatus::NotFound)
}

#[no_mangle]
pub extern "C" fn _nss_ghteam_setpwent() -> libc::c_int {
    let pid = unsafe { libc::getpid() };
    std::fs::create_dir_all(RUN_DIR).unwrap();
    let mut idx_file:File = File::create(format!("{}/{}.index",RUN_DIR,pid)).unwrap();
    idx_file.write(b"0").unwrap();
    let mut list_file:File = File::create(format!("{}/{}.list",RUN_DIR,pid)).unwrap();
    let (_,members) = CLIENT.get_team_members().unwrap();
    for member in members.values () {
        list_file.write(format!("{}\t{}\n",member.login,member.id).as_bytes()).unwrap();
    }
    libc::c_int::from(NssStatus::Success)
}

#[no_mangle]
pub extern "C" fn _nss_ghteam_getpwent_r(
    pwbuf: *mut Passwd,
    buf: *mut libc::c_char,
    buflen: libc::size_t,
    pwbufp: *mut *mut Passwd,
    ) -> libc::c_int {
    let pid = unsafe { libc::getpid() };
    let mut idx_file:File =  OpenOptions::new().read(true).write(true).open(format!("{}/{}.index",RUN_DIR,pid)).unwrap();
    let mut idx_string = String::new();
    idx_file.read_to_string(&mut idx_string).unwrap();
    let idx:usize = idx_string.parse().unwrap();
    let list = BufReader::new(File::open(format!("{}/{}.list",RUN_DIR,pid)).unwrap());
    for (i,line) in list.lines().enumerate() {
        if i!=idx { continue }
        if let Ok(l) = line {
            let mut buffer = Buffer::new(buf,buflen);
            let words:Vec<&str> = l.split("\t").collect();
            match unsafe {(*pwbuf).pack(
                &mut buffer,
                words[0],
                "x",
                words[1].parse::<usize>().unwrap() as libc::uid_t,
                CLIENT.conf.gid as libc::gid_t,
                "",
                &CLIENT.conf.home.replace("{}",words[0]),
                &CLIENT.conf.sh,
            )} {
                Ok(_) => {
                    idx_file.seek(SeekFrom::Start(0)).unwrap();
                    idx_file.write(format!("{}",idx+1).as_bytes()).unwrap();
                    unsafe {*pwbufp = pwbuf};
                    return libc::c_int::from(NssStatus::Success)
                },
                Err(_) => {
                    unsafe {*pwbufp = std::ptr::null_mut()};
                    return nix::Errno::ERANGE as libc::c_int
                }
            }
        }
    }
    libc::c_int::from(NssStatus::Unavail)
}

#[no_mangle]
pub extern "C" fn _nss_ghteam_endpwent() -> libc::c_int {
    let pid = unsafe { libc::getpid() };
    std::fs::remove_file(format!("{}/{}.index",RUN_DIR,pid)).unwrap_or(());
    std::fs::remove_file(format!("{}/{}.list",RUN_DIR,pid)).unwrap_or(());
    libc::c_int::from(NssStatus::Success)
}

#[no_mangle]
pub extern "C" fn _nss_ghteam_getspnam_r(cnameptr: *const libc::c_char,
                                         spwd: *mut Spwd,
                                         buf: *mut libc::c_char,
                                         buflen: libc::size_t,
                                         _: *mut libc::c_int) -> libc::c_int {
    let mut buffer = Buffer::new(buf,buflen);
    let cname: &CStr = unsafe {CStr::from_ptr(cnameptr)};
    let name = String::from(cname.to_str().unwrap());
    let (_,members) = CLIENT.get_team_members().unwrap();
    match members.get(&name) {
        Some(member) => {
            match unsafe {(*spwd).pack(
                    &mut buffer,
                    &member.login,
                    "!!",
                    -1,-1,-1,-1,-1,-1,0
                )} {
                Ok(_) => libc::c_int::from(NssStatus::Success),
                Err(_) => nix::Errno::ERANGE as libc::c_int
            }

        },
        None => libc::c_int::from(NssStatus::NotFound)
    }
}

#[no_mangle]
pub extern "C" fn _nss_ghteam_setspent() -> libc::c_int { libc::c_int::from(NssStatus::Success) }

#[no_mangle]
pub extern "C" fn _nss_ghteam_getspent_r() -> libc::c_int { libc::c_int::from(NssStatus::Unavail) }

#[no_mangle]
pub extern "C" fn _nss_ghteam_endspent() -> libc::c_int { libc::c_int::from(NssStatus::Success) }

#[no_mangle]
pub extern "C" fn _nss_ghteam_getgrgid_r(gid: libc::gid_t,
                                         group: *mut Group,
                                         buf: *mut libc::c_char,
                                         buflen: libc::size_t,
                                         _: *mut libc::c_int) -> libc::c_int {
    let mut buffer = Buffer::new(buf,buflen);
    let (team,members) = CLIENT.get_team_members().unwrap();
    let members:Vec<&str> = members.values().map(|m| m.login.as_str()).collect();
    if gid == CLIENT.conf.gid as libc::gid_t {
        match unsafe{(*group).pack(
                &mut buffer,
                &team.name,
                "x",
                gid,
                &members)} {
            Ok(_) => libc::c_int::from(NssStatus::Success),
            Err(_) => nix::Errno::ERANGE as libc::c_int,
        }
    } else {
        libc::c_int::from(NssStatus::NotFound)
    }
}

#[no_mangle]
pub extern "C" fn _nss_ghteam_getgrnam_r(cnameptr: *const libc::c_char,
                                         group: *mut Group,
                                         buf: *mut libc::c_char,
                                         buflen: libc::size_t,
                                         _: *mut libc::c_int) -> libc::c_int {
    let mut buffer = Buffer::new(buf,buflen);
    let cname: &CStr = unsafe {CStr::from_ptr(cnameptr)};
    let name = String::from(cname.to_str().unwrap());
    let (team,members) = CLIENT.get_team_members().unwrap();
    let members:Vec<&str> = members.values().map(|m| m.login.as_str()).collect();
    if name == CLIENT.conf.team {
        match unsafe{(*group).pack(
                &mut buffer,
                &team.name,
                "x",
                team.id as libc::gid_t,
                &members
            )} {
            Ok(_) => libc::c_int::from(NssStatus::Success),
            Err(_) => nix::Errno::ERANGE as libc::c_int
        }

    } else {
        libc::c_int::from(NssStatus::NotFound)
    }
}

#[no_mangle]
pub extern "C" fn _nss_ghteam_setgrent() -> libc::c_int { libc::c_int::from(NssStatus::Success) }

#[no_mangle]
pub extern "C" fn _nss_ghteam_getgrent_r() -> libc::c_int { libc::c_int::from(NssStatus::Unavail) }

#[no_mangle]
pub extern "C" fn _nss_ghteam_endgrent() -> libc::c_int { libc::c_int::from(NssStatus::Success) }
