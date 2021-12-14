extern crate cursive;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate fern;
extern crate chrono;

extern crate rat_error;
extern crate rat_config;


use std::thread;
use std::sync::RwLock;
use std::io::{
    self,
    BufRead,
    BufReader,
    Write,
};
use std::net::{
    IpAddr,
    SocketAddr,
    TcpStream,
    Shutdown,
};
use std::sync::mpsc::{
    channel,
    Sender,
    Receiver,
    TryRecvError,
};

use cursive::Cursive;
use cursive::CursiveExt;
use cursive::traits::*;
use cursive::align::HAlign;
use cursive::view::Scrollable;
use cursive::view::scroll::ScrollStrategy;
use cursive::event::{
    Event,
    Callback,
    EventResult,
    Key::*,
};
use cursive::views::{
    Panel,
    Dialog,
    Button,
    TextArea,
    TextView,
    NamedView,
    DummyView,
    ScrollView,
    ResizedView,
    OnEventView,
    LinearLayout,
};

use log::LevelFilter;

use rat_error::*;
use rat_config::*;


#[derive(Debug)]
enum ServerThreadCommand {
    Shutdown,
    Write(String),
}

#[derive(Debug)]
enum TuiThreadCommand {
    AddMessage(String),
    Popup(String),
    DisableChatbox,
}


fn tui_thread(
    tui_thread_rchan: Receiver<TuiThreadCommand>,
    read_thread_schan: Sender<ServerThreadCommand>
) -> thread::JoinHandle< Result<()> > {

    // Add message to chat_room
    fn add_message(siv: &mut Cursive, message: &String, name: &String) -> Result<()> {
        let mut chat_room;
        let mut scroll_chat_room;
        let should_scroll;

        // Find chat_room object
        chat_room = match siv.find_name::<LinearLayout>("chat_room") {
            Some(w) => w,
            None    => {
                warn!("Failed to find chat_room");
                return Err(RatError::Tui);
            }
        };

        scroll_chat_room = match
            siv.find_name::<
                ScrollView<
                    NamedView<
                        LinearLayout
                    >
                >
            >("scroll_chat_room")
        {
            Some(w) => w,
            None    => {
                warn!("Failed to find scroll_chat_room");
                return Err(RatError::Tui);
            }
        };

        // If last message is in view, scroll with new messages
        should_scroll = scroll_chat_room.is_at_bottom();
        if should_scroll {
            scroll_chat_room.set_scroll_strategy(ScrollStrategy::StickToBottom);
        }
 
        // Add : to name
        let mut name = name.clone();
        name.push(':');

        // Add message to chat_room
        chat_room.add_child(
            TextView::new(name)
        );
        chat_room.add_child(
            LinearLayout::horizontal()
            .child(
                DummyView
                .fixed_width(4)
            )
            .child(
                TextView::new(message)
            )
        );

        Ok(())
    }

    // Post message to server
    fn post_message(message: &String, schan: Sender<ServerThreadCommand>) -> Result<()> {
        match schan.send(ServerThreadCommand::Write(message.to_string())) {
            Ok(_) => {
                Ok(())
            },
            Err(e)  => {
                warn!("Failed to post message");
                debug!("{:?}", e);
                Err(RatError::SendCommand)
            }
        }
    }

    // Posts and adds message
    fn send_message(s: &mut Cursive) -> Result<()> {
        let mut chat_box;
        let message;

        // Get chat_box object
        chat_box = match s.find_name::<TextArea>("chat_box") {
            Some(v) => v,
            None    => {
                warn!("Failed to find chat_box");
                return Err(RatError::Tui);
            }
        };

        // Get message from chat_box
        message = chat_box.get_content().to_string();

        // Get name
        let name = match RAT_CONFIG.read() {
            Ok(v)   => v.get_str("name")?,
            Err(e)  => {
                warn!("Failed to read config");
                debug!("{:?}", e);
                return Err(RatError::ConfigLock);
            }
        };

        // Get channel (through siv itself)
        let data = s.with_user_data(|data: &mut Sender<ServerThreadCommand>| -> Sender<ServerThreadCommand> {
            return data.clone();
        });

        let schan = match data {
            Some(v) => v,
            _ => {
                warn!("Failed to read app data");
                return Err(RatError::Tui);
            }
        };

        post_message(&message, schan)?;
        add_message(s, &message, &name)?;
        chat_box.set_content("");   // Clear chat_box

        Ok(())
    }

    // Handle any commands sent from other threads
    fn handle_command(siv: &mut Cursive, command: TuiThreadCommand) -> Result<()> {
        trace!("Handling tui command: {:?}", command);
        match command {
            TuiThreadCommand::AddMessage(mut v) => {
                // Split the name from the message
                let split_index = match v.find('\n') {
                    Some(v) => v,
                    None => {
                        warn!("Failed to find name delimiter");
                        return Err(RatError::InvalidMessage(v));
                    }
                };
                let mut msg = v.split_off(split_index);
                msg.remove(0);
                let name = v;

                add_message(siv, &msg, &name)
            },
            TuiThreadCommand::Popup(v) => {
                siv.add_layer(
                    Dialog::info(v.as_str())
                );

                Ok(())
            },
            TuiThreadCommand::DisableChatbox => {
                let mut chat_box = match siv.find_name::<TextArea>("chat_box") {
                    Some(v) => v,
                    None    => {
                        warn!("Failed to find chat_box");
                        return Err(RatError::Tui);
                    }
                };

                let mut send_btn = match siv.find_name::<Button>("send_btn") {
                    Some(v) => v,
                    None    => {
                        warn!("Failed to find send_btn");
                        return Err(RatError::Tui);
                    }
                };

                chat_box.disable();
                send_btn.disable();

                match siv.focus_name("chat_room") {
                    Err(e) => {
                        warn!("Failed to focus chat_room");
                        debug!("{:?}", e);
                        return Err(RatError::Tui);
                    },
                    _ => ()
                }

                Ok(())
            }
        }
    }

    let handle;
    
    handle = thread::spawn(move || -> Result<()> {
        let mut siv = Cursive::default();
        let app_data = read_thread_schan.clone();
        siv.set_user_data(app_data);

        // Send name to server
        let name = match RAT_CONFIG.read() {
            Ok(v)   => v.get_str("name")?,
            Err(e)  => {
                error!("Failed to read config");
                debug!("{:?}", e);
                return Err(RatError::ConfigLock);
            }
        };
        post_message(&name, read_thread_schan.clone())?;

        // Triggered when enter is pressed inside chat_box
        let enter_key_event_closure = move |s: &mut Cursive| {
            match send_message(s) {
                Err(e) => {
                    warn!("Failed to send message");
                    debug!("{:?}", e);
                },
                _ => ()
            }
        };

        // Triggered when <Send> button is pressed
        let send_btn_event_closure = move |s: &mut Cursive| {
            match send_message(s) {
                Err(e) => {
                    warn!("Failed to send message");
                    debug!("{:?}", e);
                },
                _ => ()
            }
        };
        
        // Closure to run when q is pressed globally
        let closure_schan = read_thread_schan.clone();
        let quit_key_event_closure = move |s: &mut Cursive| {
            match closure_schan.send(ServerThreadCommand::Shutdown) {
                Err(e) => {
                    warn!("Failed to send shutdown command");
                    debug!("{:?}", e);
                },
                _ => ()
            }

            s.quit();
        };

        // Closure to run when the <Quit> button is pressed
        let closure_schan = read_thread_schan.clone();
        let quit_btn_event_closure = move |s: &mut Cursive| {
            match closure_schan.send(ServerThreadCommand::Shutdown) {
                Err(e) => {
                    warn!("Failed to send shutdown command");
                    debug!("{:?}", e);
                },
                _ => ()
            }

            s.quit();
        };

        // We can quit by pressing q
        siv.add_global_callback('q', quit_key_event_closure);

        siv.add_fullscreen_layer(
            Dialog::around(
                ResizedView::with_full_screen(
                    LinearLayout::vertical()
                    .child(
                        Panel::new(
                            LinearLayout::vertical()
                            .child(
                                TextView::new("Let there be chat.")
                                .h_align(HAlign::Center)
                                .full_width()
                            )
                            .with_name("chat_room")
                            .scrollable()
                            .scroll_strategy(ScrollStrategy::StickToBottom)
                            .with_name("scroll_chat_room")
                        )
                        .full_height()
                    )
                    .child(
                        Panel::new(
                            LinearLayout::horizontal()
                            .child(
                                OnEventView::new(
                                    TextArea::new() 
                                    .with_name("chat_box")
                                    .fixed_height(3)
                                    .full_width()
                                )
                                .on_pre_event_inner(    // Insert '\n' when Ctrl+n is pressed
                                    Event::CtrlChar('n'),
                                    move |view, _event| {
                                        let mut text_area;
                                        let mut content;
                                        let cursor;

                                        text_area = view
                                        .get_inner_mut()    // ResizedView
                                        .get_inner_mut()    // NamedView
                                        .get_mut();         // TextArea
                                        
                                        // Insert newline after cursor pos in content
                                        content = text_area.get_content().to_string();
                                        cursor = text_area.cursor();
                                        content.insert(cursor, '\n');
                                        text_area.set_content(content);
                                        text_area.set_cursor(cursor+1); // Move cursor after the new newline char

                                        Some(EventResult::Consumed(None))
                                    }
                                )
                                .on_pre_event_inner(    // Send message when Enter key is pressed
                                    Event::Key(Enter),
                                    move |_view, _event| {
                                        Some(EventResult::Consumed(
                                            Some(Callback::from_fn_mut(
                                                enter_key_event_closure
                                            ))
                                        ))
                                    }
                                )
                            )
                            .child(
                                LinearLayout::vertical()
                                .child(
                                    DummyView
                                    .fixed_size((6, 1))
                                )
                                .child(
                                    Button::new("Send", send_btn_event_closure)
                                    .with_name("send_btn")
                                    .fixed_size((6, 2))
                                )
                            )
                        )
                    )
                )
            )
            .title("ChatRoom")
            .h_align(HAlign::Center)
            .button("Quit", quit_btn_event_closure)
        );
        
        // Main TUI loop
        siv.refresh();
        loop {
            // Check for commands from other threads
            match tui_thread_rchan.try_recv() {
                Ok(v) => {
                    handle_command(&mut siv, v)?;
                },
                Err(TryRecvError::Empty) => {
                    ()
                },
                Err(e) => {
                    error!("Failed to receive tui command");
                    debug!("{:?}", e);
                    return Err(RatError::from(e));
                }
            }

            siv.refresh();
            siv.step();
            if !siv.is_running() {
                break;
            }
        }

        Ok(())
    });

    return handle;
}

fn server_thread(
    mut stream: TcpStream,
    server_thread_rchan: Receiver<ServerThreadCommand>,
    tui_thread_schan: Sender<TuiThreadCommand>
) -> thread::JoinHandle< Result<()> > {
    const DELIMITER: u8 = 0x04; // EOT

    // Handle commands from other threads
    fn handle_command(stream: &mut TcpStream, command: ServerThreadCommand) -> Result<()> {
        trace!("Handling server command: {:?}", command);
        match command {
            ServerThreadCommand::Shutdown => {
                stream.shutdown(Shutdown::Both)?;
                Ok(())
            },
            ServerThreadCommand::Write(v) => {
                // Add EOT
                let mut msg = v;
                msg.push(DELIMITER.into());

                stream.write_all(msg.as_bytes())?;
                Ok(())
            }
        }
    }

    let handle;
    
    handle = thread::spawn(move || -> Result<()> {
        let mut num_read;
        let mut buffer;

        buffer = BufReader::new(stream.try_clone()?);
        loop {
            // Check for commands from other threads
            match server_thread_rchan.try_recv() {
                Ok(v) => {
                    handle_command(&mut stream, v)?;
                },
                Err(TryRecvError::Empty) => {
                    ()
                },
                Err(e) => {
                    error!("Failed to receive server command");
                    debug!("{:?}", e);
                    return Err(RatError::from(e));
                }
            }

            let mut msg: Vec<u8>;
            let string_msg: String;

            msg = Vec::new();
            num_read = match buffer.read_until(DELIMITER, &mut msg) { // Read until EOT or EOF
                Ok(v)       => v,
                Err(e)  if e.kind() == io::ErrorKind::WouldBlock => {
                    // Not EOF, but no new messages
                    continue;
                },
                Err(e)      => {
                    error!("Failed to read from client");
                    debug!("{:?}", e);
                    return Err(RatError::from(e));
                }
            };

            if num_read == 0 {  // if buffer has reached EOF
                break;
            }

            // New message
            msg.pop();  // Remove delimeter
            string_msg = String::from_utf8(msg)?;

            trace!("Add Message: {:?}", string_msg);
            match tui_thread_schan.send(
                TuiThreadCommand::AddMessage(string_msg)
            ) {
                Err(e) => {
                    warn!("Failed to send AddMessage command");
                    debug!("{:?}", e);
                },
                _ => ()
            }
        }

        Ok(())
    });

    return handle;
}

fn connect_to_dst() -> Result<TcpStream> {
    let dst_ip:     IpAddr;
    let dst_port:   u16;
    let stream;
    
    let config = match RAT_CONFIG.read() {
        Ok(v)   => v,
        Err(e)  => {
            error!("Failed to read config");
            debug!("{:?}", e);
            return Err(RatError::ConfigLock);
        }
    };

    dst_ip = config.get_str("dst_ip")?.parse()?;
    dst_port = config.get("dst_port")?;

    stream = TcpStream::connect(
        SocketAddr::new(
            dst_ip,
            dst_port
        )
    )?;
    stream.set_nonblocking(true)?;

    return Ok(stream);
}

lazy_static!{
    static ref RAT_CONFIG: RwLock<Config> = RwLock::new(Config::default());
}

fn main() {
    let stream;
    let tui_thread_handle;
    let server_thread_handle;

    match set_client_config_defaults(&RAT_CONFIG) {
        Err(ref e) => {
            exit_with_error(e);
        },
        _ => ()
    }

    match parse_client_config_file(&RAT_CONFIG) {
        Err(ref e) => {
            exit_with_error(e);
        },
        _ => ()
    }

    // Initialize argument parsing with clap
    match parse_client_args(&RAT_CONFIG) {
        Err(ref e) => {
            exit_with_error(e);
        },
        _ => ()
    }

    // Read config
    let log_level_stderr;
    let log_level_file;
    {
        let config = match RAT_CONFIG.read() {
            Ok(v)   => v,
            Err(_)  => {
                exit_with_error(&RatError::ConfigLock);
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
                    &RatError::UnknownOption(
                        format!("log_level_stderr: {}", s).to_string()
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
                    &RatError::UnknownOption(
                        format!("log_level_file: {}", s).to_string()
                    )
                );
                unreachable!();
            }
        };
        log_level_file = __log_level_file;
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

    match get_client_log_filename() {
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
                    .level_for("cursive", LevelFilter::Warn)
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

    // Start TCP connection.
    match connect_to_dst() {
        Ok(v)       => {
            stream = v;
        },
        Err(ref e)  => {
            exit_with_error(e);
            unreachable!(); // We should never get here.
        }
    }

    debug!("Connected: {:?}", stream);

    // Create thread communication channels
    let (server_thread_schan, server_thread_rchan) = channel::<ServerThreadCommand>();
    let (tui_thread_schan, tui_thread_rchan)  = channel::<TuiThreadCommand>();

    // Start threads
    tui_thread_handle = tui_thread(tui_thread_rchan, server_thread_schan);                          // Cursive TUI
    server_thread_handle = server_thread(stream, server_thread_rchan, tui_thread_schan.clone());    // Handle data to and from the server

    // Gracefully exit
    match server_thread_handle.join() {
        Err(e) => {
            debug!("Read thread error: {:?}", e);

            match tui_thread_schan.send(
                TuiThreadCommand::Popup("Read thread panicked.".to_string())
            ) {
                Err(e) => {
                    warn!("Failed to send popup command");
                    debug!("{:?}", e);
                },
                _ => ()
            }
        },
        _ => {
            info!("Connection closed.");

            match tui_thread_schan.send(
                TuiThreadCommand::Popup("Connection has been closed.".to_string())
            ) {
                Err(e) => {
                    warn!("Failed to send popup command");
                    debug!("{:?}", e);
                },
                _ => ()
            }

            match tui_thread_schan.send(
                TuiThreadCommand::DisableChatbox
            ) {
                Err(e) => {
                    warn!("Failed to send DiableChatbox command");
                    debug!("{:?}", e);
                },
                _ => ()
            }
        }
    }

    match tui_thread_handle.join() {
        Err(e) => {
            debug!("TUI thread error: {:?}", e);

            exit_with_error(
                &RatError::Tui
            );
        },
        _ => {
            info!("Exiting normally.");
        }
    }
}
