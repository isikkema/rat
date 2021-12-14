extern crate uuid;

extern crate rat_error;


use std::net::TcpStream;
use std::ops::Deref;
use std::io::{
    self,
    BufRead,
    BufReader,
};
use std::sync::{
    Arc,
    Mutex,
};

use uuid::Uuid;

use rat_error::*;

// Split Client into Client and ClientInner so I can impl functions for Client
// while still keeping the data in an Arc for use across threads.
#[derive(Debug)]
pub struct ClientInner {
    pub id: Uuid,
    pub name: String,
    pub stream: Mutex<TcpStream>,
}

#[derive(Clone)]
#[derive(Debug)]
pub struct Client {
    inner: Arc<ClientInner>
}


// On deref, return the inner data
impl Deref for Client {
    type Target = Arc<ClientInner>;
    
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Client {
    pub fn new(stream: TcpStream) -> Client {
        Client {
            inner: Arc::new(
                ClientInner {
                    id: Uuid::new_v4(),
                    name: String::new(),    // Empty name. Must call set_name() later
                    stream: Mutex::new(stream),
                }
            )
        }
    }

    // Can only call this while there is ONE reference to the data.
    // This will fail otherwise.
    pub fn set_name(&mut self, name: String) {
        Arc::get_mut(&mut self.inner).unwrap().name = name;
    }

    // Returns a string message of stream data with a terminator value of EOT
    // Returns "" if the stream is closed
    // Returns any error if encountered, including WouldBlock
    pub fn read(&self) -> Result<String> {
        const DELIMITER: u8 = 0x04;  // EOT

        // Read stream
        let stream = self.inner.stream.lock().unwrap();
        let mut buffer = BufReader::new(stream.try_clone()?);
        let mut msg: Vec<u8>;
        let string_msg: String;
        let num_read;
        
        msg = Vec::new();
        num_read = match buffer.read_until(DELIMITER, &mut msg) { // Read until EOT or EOF
            Ok(v)       => v,
            Err(e)      => {
                return Err(RatError::from(e));
            }
        };

        // Drop lock now in case the write thread is waiting for it
        drop(buffer);
        drop(stream);

        if num_read == 0 {  // if buffer has reached EOF
            return Ok(String::from(""));
        }

        // New message
        string_msg = String::from_utf8(msg)?;

        return Ok(string_msg);
    }

    // Reads until a string or an error other than WouldBlock is returned
    pub fn read_block(&self) -> Result<String> {
        loop {
            match self.read() {
                Ok(v) => {
                    return Ok(v);
                },
                Err(RatError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                    continue;
                },
                Err(e) => {
                    return Err(RatError::from(e));
                }
            };
        }
    }
}

