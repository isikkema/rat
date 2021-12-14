extern crate config;


use std::{
    io,
    fmt,
    num,
    net,
    error,
    string,
    result,
    convert,
    process,
};
use std::sync::mpsc;


#[derive(Debug)]
pub enum RatError {
    Io(io::Error),
    ParseInt(num::ParseIntError),
    ParseAddr(net::AddrParseError),
    FromUtf8(string::FromUtf8Error),
    Config(config::ConfigError),
    TryRecv(mpsc::TryRecvError),

    Tui,
    SendCommand,
    ConfigLock,
    ConfigDir,
    InvalidMessage(String),
    UnknownOption(String),
    Custom(String),
}

pub type Result<T> = result::Result<T, RatError>;


impl fmt::Display for RatError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RatError::Io(ref e)         => write!(f, "IO Error: {}", e),
            RatError::ParseInt(ref e)   => write!(f, "ParseInt Error: {}", e),
            RatError::ParseAddr(ref e)  => write!(f, "ParseAddr Error: {}", e),
            RatError::FromUtf8(ref e)   => write!(f, "FromUtf8 Error: {}", e),
            RatError::Config(ref e)     => write!(f, "Config Error: {}", e),
            RatError::TryRecv(ref e)    => write!(f, "TryRecv Error: {}", e),

            RatError::Tui                   => write!(f, "Rat Error: TUI failed"),
            RatError::SendCommand           => write!(f, "Rat Error: Failed to send command"),
            RatError::ConfigLock            => write!(f, "Rat Error: Failed to unlock config"),
            RatError::ConfigDir             => write!(f, "Rat Error: Failed to determine config directory"),
            RatError::InvalidMessage(ref e) => write!(f, "Rat Error: Invalid message: {}", e),
            RatError::UnknownOption(ref e)  => write!(f, "Rat Error: Unknown option: {}", e),
            RatError::Custom(ref e)         => write!(f, "Rat Error: {}", e),
        }
    }
}


impl error::Error for RatError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            RatError::Io(ref e)         => Some(e),
            RatError::ParseInt(ref e)   => Some(e),
            RatError::ParseAddr(ref e)  => Some(e),
            RatError::FromUtf8(ref e)   => Some(e),
            RatError::Config(ref e)     => Some(e),
            RatError::TryRecv(ref e)    => Some(e),
            _                           => None,
        }
    }
}


impl convert::From<io::Error> for RatError {
    fn from(err: io::Error) -> RatError {
        RatError::Io(err)
    }
}

impl convert::From<num::ParseIntError> for RatError {
    fn from(err: num::ParseIntError) -> RatError {
        RatError::ParseInt(err)
    }
}

impl convert::From<net::AddrParseError> for RatError {
    fn from(err: net::AddrParseError) -> RatError {
        RatError::ParseAddr(err)
    }
}

impl convert::From<string::FromUtf8Error> for RatError {
    fn from(err: string::FromUtf8Error) -> RatError {
        RatError::FromUtf8(err)
    }
}

impl convert::From<config::ConfigError> for RatError {
    fn from(err: config::ConfigError) -> RatError {
        RatError::Config(err)
    }
}

impl convert::From<mpsc::TryRecvError> for RatError {
    fn from(err: mpsc::TryRecvError) -> RatError {
        RatError::TryRecv(err)
    }
}


pub fn exit_with_error(e: &RatError) {
    eprintln!("{}", e);
    process::exit(1);
}

