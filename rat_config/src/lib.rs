extern crate config;
extern crate directories;
#[macro_use]
extern crate clap;

extern crate rat_error;

use std::fs;
use std::sync::RwLock;

use directories::ProjectDirs;

use clap::App;

use rat_error::RatError;
use rat_error::Result as Result;


pub use config::Config;


const QUALIFIER:        &str = "isikkema";  // <--  idk what to put for these.
const ORGANIZATION:     &str = "isikkema";  // <--
const APPLICATION:      &str = "Rat";

const CLIENT_LOG_FILENAME:      &str = "Client.log";
const CLIENT_CONFIG_FILENAME:   &str = "ClientConfig.toml";
const SERVER_LOG_FILENAME:      &str = "Server.log";
const SERVER_CONFIG_FILENAME:   &str = "ServerConfig.toml";


fn get_filename(filename: &str) -> Result<String> {
    let path;
    let mut file;

    // Get path to config dir
    path = match ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION) {
        Some(v) => v,
        None    => {
            return Err(RatError::ConfigDir);
        }
    };

    // Get config dir
    let path = path.config_dir();

    // Create all directories in path to dir if it doesn't exist
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    
    // Create config file in dir if it doesn't exist
    file = path.to_path_buf();
    file.push(filename);
    if !file.exists() {
        fs::File::create(&file)?;
    }

    return Ok(file.to_str().unwrap().to_string());
}

fn parse_config_file(config: &RwLock<Config>, config_filename: String) -> Result<()> {
    match config.write() {
        Ok(mut v)   => {
            v.merge(
                config::File::with_name(
                    config_filename.as_str()
                )
            )?;

            Ok(())
        },
        Err(_)  => {
            Err(RatError::ConfigLock)
        }
    }
}

pub fn get_client_log_filename() -> Result<String> {
    return get_filename(CLIENT_LOG_FILENAME);
}

fn get_client_config_filename() -> Result<String> {
    return get_filename(CLIENT_CONFIG_FILENAME);
}

pub fn parse_client_config_file(config: &RwLock<Config>) -> Result<()> {
    return parse_config_file(config, get_client_config_filename()?);
}

pub fn set_client_config_defaults(config: &RwLock<Config>) -> Result<()> {
    let mut config = match config.write() {
        Ok(v)   => v,
        Err(_)  => {
            return Err(RatError::ConfigLock);
        }
    };

    config.set_default("log_level_stderr", "error".to_string())?;
    config.set_default("log_level_file", "debug".to_string())?;
    config.set_default("dst_port", 18888)?;
    config.set_default("name", "anonymous".to_string())?;

    Ok(())
}

pub fn parse_client_args(config: &RwLock<Config>) -> Result<()> {
    let yaml = load_yaml!("client_cli.yml");
    let matches = App::from_yaml(yaml).get_matches();

    let mut config = match config.write() {
        Ok(v)   => v,
        Err(_)  => {
            return Err(RatError::ConfigLock);
        }
    };
    
    match matches.value_of("dst_ip") {
        Some(v) => {
            config.set("dst_ip", v.to_string())?;
        },
        None => ()
    }

    match matches.value_of("dst_port") {
        Some(v) => {
            config.set("dst_port", v.parse::<i64>()?)?;
        },
        None => ()
    }
    
    match matches.value_of("name") {
        Some(v) => {
            config.set("name", v.to_string())?;
        },
        None => ()
    }

    Ok(())
}

pub fn get_server_log_filename() -> Result<String> {
    return get_filename(SERVER_LOG_FILENAME);
}

fn get_server_config_filename() -> Result<String> {
    return get_filename(SERVER_CONFIG_FILENAME);
}

pub fn parse_server_config_file(config: &RwLock<Config>) -> Result<()> {
    return parse_config_file(config, get_server_config_filename()?);
}

pub fn set_server_config_defaults(config: &RwLock<Config>) -> Result<()> {
    let mut config = match config.write() {
        Ok(v)   => v,
        Err(_)  => {
            return Err(RatError::ConfigLock);
        }
    };

    config.set_default("log_level_stderr", "info".to_string())?;
    config.set_default("log_level_file", "debug".to_string())?;
    config.set_default("src_ip", "0.0.0.0".to_string())?;
    config.set_default("src_port", 18888)?;

    Ok(())
}

pub fn parse_server_args(config: &RwLock<Config>) -> Result<()> {
    let yaml = load_yaml!("server_cli.yml");
    let matches = App::from_yaml(yaml).get_matches();

    let mut config = match config.write() {
        Ok(v)   => v,
        Err(_)  => {
            return Err(RatError::ConfigLock);
        }
    };
    
    match matches.value_of("src_ip") {
        Some(v) => {
            config.set("src_ip", v.to_string())?;
        },
        None => ()
    }

    match matches.value_of("src_port") {
        Some(v) => {
            config.set("src_port", v.parse::<i64>()?)?;
        },
        None => ()
    }
    
    Ok(())
}

