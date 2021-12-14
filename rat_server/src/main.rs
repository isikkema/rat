#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate fern;
extern crate chrono;

extern crate rat_error;
extern crate rat_config;

mod client;


use std::thread;
use std::sync::{
    RwLock,
    Arc,
};
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};
use std::net::{
    IpAddr,
    SocketAddr,
    TcpListener,
};
use std::io::{
    self,
    Write,
};
use std::sync::mpsc::{
    channel,
    Sender,
    Receiver,
    TryRecvError,
};

use log::LevelFilter;

use rat_error::*;
use rat_config::*;

use crate::client::*;


#[derive(Debug)]
enum WriteThreadCommand {
    WriteChat(String, Client),
    WriteJoinEvent(Client),
    WriteLeaveEvent(Client),
    AddClient(Client),
    RemoveClient(Client),
}


fn write_thread(
    write_thread_rchan: Receiver<WriteThreadCommand>,
    write_thread_running_flag: Arc<AtomicBool>
) -> thread::JoinHandle<Result<()>> {
    fn handle_command(
        command: WriteThreadCommand,
        clients: &mut Vec<Client>
    ) -> Result<()> {
        const DELIMITER: char = '\u{4}';

        trace!("Handling Command: {:?}", command);
        match command {
            // Write chat msg to all Clients
            WriteThreadCommand::WriteChat(msg, from_client) => {
                for client in clients.iter() {
                    if client.id == from_client.id {
                        continue;
                    }

                    let full_msg = format!("{}\n{}\n{}", "Chat", from_client.name, msg);
                    let mut stream = match client.stream.lock() {
                        Ok(v) => v,
                        Err(_) => {
                            return Err(RatError::Custom("Client lock poisoned.".to_string()));
                        }
                    };

                    trace!("Sending message: {:?}", full_msg);
                    match stream.write_all(full_msg.as_bytes()) {
                        Err(e) => {
                            warn!("Failed to write");
                            debug!("{:?}", e);
                        },
                        _ => ()
                    }
                }
            },
            // Write Join event to all Clients
            WriteThreadCommand::WriteJoinEvent(from_client) => {
                for client in clients.iter() {
                    if client.id == from_client.id {
                        continue;
                    }

                    let full_msg = format!(
                        "{}\n{}{}",
                        "JoinEvent",
                        from_client.name,
                        DELIMITER
                    );

                    let mut stream = match client.stream.lock() {
                        Ok(v) => v,
                        Err(_) => {
                            return Err(RatError::Custom("Client lock poisoned.".to_string()));
                        }
                    };

                    trace!("Sending message: {:?}", full_msg);
                    match stream.write_all(full_msg.as_bytes()) {
                        Err(e) => {
                            warn!("Failed to write");
                            debug!("{:?}", e);
                        },
                        _ => ()
                    }
                }
            },
            WriteThreadCommand::WriteLeaveEvent(from_client) => {
                for client in clients.iter() {
                    if client.id == from_client.id {
                        continue;
                    }

                    let full_msg = format!(
                        "{}\n{}{}",
                        "LeaveEvent",
                        from_client.name,
                        DELIMITER
                    );

                    let mut stream = match client.stream.lock() {
                        Ok(v) => v,
                        Err(_) => {
                            return Err(RatError::Custom("Client lock poisoned.".to_string()));
                        }
                    };

                    trace!("Sending message: {:?}", full_msg);
                    match stream.write_all(full_msg.as_bytes()) {
                        Err(e) => {
                            warn!("Failed to write");
                            debug!("{:?}", e);
                        },
                        _ => ()
                    }
                }
            },
            // Add Client to list
            WriteThreadCommand::AddClient(client) => {
                trace!("Adding Client.");
                clients.push(client);
            },
            // Remove Client from list
            WriteThreadCommand::RemoveClient(rem_client) => {
                trace!("Removing Client.");
                for (i, client) in clients.iter().enumerate() {
                    if client.id == rem_client.id {
                        clients.swap_remove(i);
                        info!("Removed Client!");
                        break;
                    }
                }
            },
        }

        trace!("Finished Command.");

        Ok(())
    }

    let handle = thread::spawn(move || -> Result<()> {
        let mut clients = Vec::new();
        let rv;

        loop {
            match write_thread_rchan.try_recv() {
                Ok(v) => {
                    match handle_command(v, &mut clients) {
                        Err(e) => {
                            warn!("Failed to handle command.");
                            debug!("{:?}", e);

                            rv = Err(e);
                            break;
                        },
                        _ => ()
                    }
                },
                Err(TryRecvError::Empty) => {
                    continue;
                },
                Err(e) => {
                    warn!("Failed to receive command.");
                    debug!("{:?}", e);

                    rv = Err(RatError::from(e));
                    break;
                }
            }
        }

        // Flag that the thread has finished running
        write_thread_running_flag.store(false, Ordering::Relaxed);
        
        rv
    });

    return handle;
}

fn read_thread(
    client: Client,
    write_thread_schan: Sender<WriteThreadCommand>
) -> thread::JoinHandle<Result<()>> {
    let handle = thread::spawn(move || -> Result<()> {
        let mut string_msg;

        loop {
            // read() instead of read_block() because we don't want to hog
            // the lock. This gives the write_thread a chance to use it if
            // it needs.
            string_msg = match client.read() {
                Ok(v) => v,

                Err(RatError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                    continue;
                },
                Err(e) => {
                    error!("Failed to read from client.");
                    debug!("{:?}", e);
                    return Err(RatError::from(e));
                }
            };
            
            // Check for EOF
            if string_msg.len() == 0 {
                // Stream is closed. Remove Client from write_thread list
                match write_thread_schan.send(
                    WriteThreadCommand::RemoveClient(client.clone())
                ) {
                    Err(e) => {
                        warn!("Failed to send RemoveClient command.");
                        debug!("{:?}", e);
                    },
                    _ => ()
                }

                match write_thread_schan.send(
                    WriteThreadCommand::WriteLeaveEvent(client.clone())
                ) {
                    Err(e) => {
                        warn!("Failed to send WriteLeaveEvent command.");
                        debug!("{:?}", e);
                    },
                    _ => ()
                }

                break;
            }

            debug!("Got message: {:?}", string_msg);

            // Post message
            match write_thread_schan.send(
                WriteThreadCommand::WriteChat(string_msg, client.clone())
            ) {
                Err(e) => {
                    warn!("Failed to send Write command.");
                    debug!("{:?}", e);
                },
                _ => ()
            }
        }

        Ok(())
    });

    return handle;
}

lazy_static!{
    static ref RAT_CONFIG: RwLock<Config> = RwLock::new(Config::default());
}

fn main() {
    // Load config
    match set_server_config_defaults(&RAT_CONFIG) {
        Err(ref e) => {
            exit_with_error(e);
        },
        _ => ()
    }

    match parse_server_config_file(&RAT_CONFIG) {
        Err(ref e) => {
            exit_with_error(e);
        },
        _ => ()
    }

    // Initialize argument parsing with clap
    match parse_server_args(&RAT_CONFIG) {
        Err(ref e) => {
            exit_with_error(e);
        },
        _ => ()
    }

    // Read config
    let log_level_stderr;
    let log_level_file;
    let src_ip;
    let src_port;
    {
        let config = match RAT_CONFIG.read() {
            Ok(v)   => v,
            Err(ref e)  => {
                exit_with_error(&RatError::Custom(e.to_string()));
                unreachable!();
            }
        };

        // log_level_stderr
        let mut __log_level_stderr = match config.get_str("log_level_stderr") {
            Ok(v) => v,
            Err(e) => {
                exit_with_error(&RatError::from(e));
                unreachable!();
            }
        };
        __log_level_stderr.make_ascii_lowercase();
        let __log_level_stderr = match __log_level_stderr.as_str() {
            "error" => LevelFilter::Error,
            "warn"  => LevelFilter::Warn,
            "info"  => LevelFilter::Info,
            "debug" => LevelFilter::Debug,
            "trace" => LevelFilter::Trace,
            s => {
                exit_with_error(
                    &RatError::Custom(
                        format!("Unknown log_level: {}", s).to_string()
                    )
                );
                unreachable!();
            }
        };
        log_level_stderr = __log_level_stderr;

        // log_level_file
        let mut __log_level_file = match config.get_str("log_level_file") {
            Ok(v) => v,
            Err(e) => {
                exit_with_error(&RatError::from(e));
                unreachable!();
            }
        };
        __log_level_file.make_ascii_lowercase();
        let __log_level_file = match __log_level_file.as_str() {
            "error" => LevelFilter::Error,
            "warn"  => LevelFilter::Warn,
            "info"  => LevelFilter::Info,
            "debug" => LevelFilter::Debug,
            "trace" => LevelFilter::Trace,
            s => {
                exit_with_error(
                    &RatError::Custom(
                        format!("Unknown log_level: {}", s).to_string()
                    )
                );
                unreachable!();
            }
        };
        log_level_file = __log_level_file;

        // src_ip
        let __src_ip = match config.get_str("src_ip") {
            Ok(v) => v,
            Err(e) => {
                exit_with_error(&RatError::from(e));
                unreachable!();
            }
        };
        let __src_ip = match __src_ip.parse::<IpAddr>() {
            Ok(v) => v,
            Err(e) => {
                exit_with_error(&RatError::from(e));
                unreachable!();
            }
        };
        src_ip = __src_ip;

        // src_port
        let __src_port = match config.get("src_port") {
            Ok(v) => v,
            Err(e) => {
                exit_with_error(&RatError::from(e));
                unreachable!();
            }
        };
        src_port = __src_port;
    }

    // Initialize logging
    let mut base_log = fern::Dispatch::new();

    let stderr_log = fern::Dispatch::new()
    .format(|out, message, record| {
        out.finish(format_args!(
            "{}[{}] {}",
            chrono::Local::now().format("[%H:%M:%S]"),
            record.level(),
            message
        ))
    })
    .level(log_level_stderr)
    .chain(io::stderr());
    base_log = base_log.chain(stderr_log);

    match get_server_log_filename() {
        Ok(v) => {
            let log_filename = v;

            match fern::log_file(log_filename) {
                Ok(v) => {
                    let log_file = v;
                    let file_log = fern::Dispatch::new()
                    .format(|out, message, record| {
                        out.finish(format_args!(
                            "{}[{}][{}] {}",
                            chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                            record.target(),
                            record.level(),
                            message
                        ))
                    })
                    .level(log_level_file)
                    .chain(log_file);

                    base_log = base_log.chain(file_log);
                },
                Err(e) => {
                    eprintln!("Failed to get log file: {:?}", e);
                }
            }
        },
        Err(e) => {
            eprintln!("Failed to get log filename: {:?}", e);
        }
    }

    match base_log.apply() {
        Err(e) => {
            eprintln!("Failed to initialize log: {:?}", e);
        },
        _ => ()
    }

    info!("Log initialized.");

    let listener = match TcpListener::bind(
        SocketAddr::new(
            src_ip,
            src_port
        )
    ) {
        Ok(v) => v,
        Err(e) => {
            exit_with_error(&RatError::from(e));
            unreachable!();
        }
    };

    let (write_thread_schan, write_thread_rchan) = channel::<WriteThreadCommand>();
    let write_thread_running_flag = Arc::new(AtomicBool::new(true));
    let write_thread_handle = write_thread(write_thread_rchan, write_thread_running_flag.clone());

    // Listen for connections
    let mut read_handles = Vec::new();
    for stream in listener.incoming() {
        // Check if write thread is still running
        if !write_thread_running_flag.load(Ordering::Relaxed) {
            match write_thread_handle.join() {
                Err(e) => {
                    debug!("Write Thread Error: {:?}", e);
                },
                _ => ()
            }

            break;
        }

        // Accept stream
        let stream = match stream {
            Ok(v) => v,
            Err(e) => {
                warn!("Failed to accept connection.");
                debug!("{:?}", e);
                continue;
            }
        };

        info!("New connection!");
        debug!("{:?}", stream);

        // Set non-blocking
        match stream.set_nonblocking(true) {
            Err(e) => {
                warn!("Failed to modify connection.");
                debug!("{:?}", e);
                continue;
            },
            _ => ()
        };

        // Get name
        let mut client = Client::new(stream);
        let name = match client.read_block() {
            Ok(mut v) => {
                v.pop();    // Remove EOT delimiter
                v
            },
            Err(e) => {
                warn!("Failed to get name.");
                debug!("{:?}", e);
                continue;
            }
        };

        debug!("Got name: {}", name);
        client.set_name(name);
        let client = client;    // Redeclare as non-mutable

        // Add Client to write_thread's list
        match write_thread_schan.send(
            WriteThreadCommand::AddClient(client.clone())
        ) {
            Err(e)  => {
                warn!("Failed to add client.");
                debug!("{:?}", e);
                continue;
            },
            _ => ()
        }

        // Send Join Event
        match write_thread_schan.send(
            WriteThreadCommand::WriteJoinEvent(client.clone())
        ) {
            Err(e) => {
                warn!("Failed to send join event.");
                debug!("{:?}", e);
                continue;   // The fact that the other clients don't get
                            // the event isn't so bad.
                            // It's just that an error here means that
                            // the write thread can't receive ANY commands
                            // anymore so why bother?
            },
            _ => ()
        }

        // Start Client's read_thread
        read_handles.push(
            read_thread(client, write_thread_schan.clone())
        );

        info!("Added new client!");
    }
}

